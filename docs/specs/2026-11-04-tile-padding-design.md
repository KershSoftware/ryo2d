# Big tiles at their own size: tile padding by sampled level

**Status:** design, approved in conversation on 2026-11-04. It follows the grid-shapes work and comes before hex collision and pathfinding.

## Why

Every tileset draws from a padded copy (`src/core/tilemap/padded.rs`), so that no filter and no smaller copy (mip level) of the texture ever reads a neighbouring tile at any zoom. The copy gets that guarantee from room. Each tile sits in a power-of-two cell of side npot(T + max(T, 8)) texels, where T is the longer side of the tileset's largest tile. Level k reads only the tile's own cell while 2^k is no more than the room past the tile, so the room has to be as wide as the tile itself.

That room is cheap for small tiles and ruinous for big ones:
- A 16 px tileset pays 4 times its texels.
- A tile just past a power of two (129 px, a 512 px cell) pays nearly 16 times.
- Kenney's Sketch Town pictures (256 x 352, 1024 px cells) pay 11.6 times.

Measured on the `tile-grids` demo at the art's own size:
- Sketch Town's ground and props took 192 and 160 MiB.
- The Hexagon Pack sheet took 225 MiB.
- That's about 577 MiB of GPU memory from about 17 MB of decoded art, and 0.2 to 0.4 s of padding work per landing.

The demo now halves its art to stay usable. Any game with big isometric or hex art pays this cost, or hits the "too big to pad" refusal, until it is fixed. `docs/backlog/tilemap.md` records it as a Limit.

The owner asked for what a professional implementation would handle.

## Goals

1. **The guarantee stays.** No tile shows colour from a neighbouring tile in its tileset at any zoom where the tile is drawn more than a few pixels wide. This holds on every grid, on both render targets (smooth and pixel-art), with either filter, for flipped, turned (hex 60° and 120°) and animated tiles, for tile objects, and for y-sorted tiles.
2. **Memory close to the art's own size.** With its smaller copies, the empty cells of its last row and any rounding included, a typical tileset's padded copy takes 1.4 to 1.7 times its decoded art, and none needs more than about 2.5 times (today: 4 to 16 times). The demo's three tilesets at full size take about 25 to 30 MB together, down from 577 MiB. Small tiles gain too: a 16 px tileset takes a quarter of what it takes now.
3. **Padding work shrinks with the memory**, so maps with big art land sooner and big tilesets are refused far less often.
4. **The web behaves the same.** Nothing here needs a GPU feature that WebGL2 lacks.
5. **Nothing else changes how it draws.** Sprites, text, UI, image layers and every non-tile image produce the same pixels as before. At zoom 1 and above, tiles produce the same pixels as before too, apart from rounding (see "How we know it works").

## Non-goals

- **Texture arrays** (each tile its own layer of a stacked texture, as bevy_ecs_tilemap does). This is the textbook alternative and equally clean, but it needs a second drawing path, and WebGL2 caps an array at 256 layers. Decided against.
- Compressed GPU texture formats (BCn, ASTC, ETC2): another memory lever, for later.
- Changing how sprites, atlas images or glyphs sample, or the 8-texel border that atlas images and every other texture of its own keep. Only padded tilesets in a texture of their own lose it (section 1).
- A cap on the room with a fixed quality floor (the backlog's first proposal). The approach below keeps tiles clean much further out for less memory.

## Anti-goals

These would make the work a failure even if the memory drops:
- A seam, a gap or a neighbour's colour on any tile drawn more than a few pixels wide. That includes the existing seam tests, the odd-size isometric raster and the half-pixel staggered steps.
- Any pixel change on a page without tiles. The scripted tour comparison must show non-tile pages byte-identical to main.
- A frame-time regression. The static-frame, edit, load and stream guards in `benches/tilemap.rs` keep their held targets.

## How it works now

- **The layout** (`padded.rs`, `layout()`): `room = side.max(MIN_ROOM)` and `cell = (side + room).next_power_of_two()`, with square cells. A margin makes the texture's border (`atlas::BORDER`, 8 texels) plus the margin a whole cell, so cells start on multiples of their side and stay whole texels down to one texel a cell.
- **Where the copy goes:** a padded image over 256 px is a texture of its own with a full mip chain. A smaller one may sit in the shared atlas (2048 px pages, 4 levels, slots on 8-texel multiples).
- **Drawing:** tile chunks draw with `vs_tile` (`shaders/core/tile.wgsl`) and the sprite's fragment stages (`fs_base`, `fs_base_premultiplied`, `fs_base_linear`). These sample with `sample_masked` (`shaders/include/sample.wgsl`): transparent outside `uv_bounds` (the tile's padded cell), clamped half a full-size texel inside it, and a level the hardware picks from the derivatives (`textureSampleGrad`).
- **Tile objects** draw as `SpriteInstance`s through the sprite path (`object_instance`, `render/tiles.rs`).
- **The pixel-art target** samples with the nearest sampler, whose `lod_max_clamp` keeps it on the full-size level.

## The design

### 1. Each tile gets a cell only slightly bigger than itself

Each tile gets a number of clean levels, K, and a cell of its own size: the tile rounded up, on each axis separately, to a multiple of 2^K texels. Cells are no longer square, nor powers of two, nor one size per tileset.

**Cells of their own size (the owner's decision).** In a Tiled image collection each picture keeps its own size, so one 512 px castle and fifty 32 px flowers take about 1.6 MB with their smaller copies. If every picture took the castle's cell, they would take about 53 MB, and today they take about 214 MB.

Every cell starts on a multiple of its own 2^K. The texture's sides are multiples of the largest 2^K among its cells, so every level up to that one halves the texture exactly. Each cell is then whole texels at every level up to its K, and no texel at those levels holds two tiles. The room past a tile is only the rounding.

A tileset whose tiles share a size packs them in a grid, as now. Mixed sizes are packed by the builder's choice of method (shelves by height, for instance), each cell on its alignment.

**An animation's frames** each take a cell of the size of the animation's largest frame. They are laid out so the shader steps from frame to frame as it does now. A frame draws as it does now.

**Everything that reads the layout's one cell size** reads the tile's own cell, or the tileset's largest tile, instead. That covers UVs and flips, the culling margins, `may_spill`, the animation steps and the "too big to pad" check.

**A padded tileset in a texture of its own carries no border (my call).** Today every texture of its own gets the atlas's 8-texel border, and a margin then makes border plus margin a whole cell. With cells aligned to 2^K, that margin would be a ring up to 2^K texels wide on every side, which alone would push the demo's Sketch Town tilesets to 2.4 times their art. A padded tileset doesn't need the border: the texture's edge clamps (`ClampToEdge`), and the shader keeps every read inside its tile's cell. So its texture starts with cell (0, 0) at texel 0. A padded image small enough for the shared atlas keeps the border its slot needs, which is already a multiple of 8, and so of 2^K for K ≤ 3. Every other image keeps its border as now.

**The number of columns** is whichever gives the smallest texture within the device's largest side, not the square root of the cell count (builder's call how). For Sketch Town's 14 ground pictures in 256 x 384 cells, 7 columns of 2 rows take 1792 x 768 texels: 1.45 times the art with its smaller copies, against 1.66 times for 4 x 4.

**The rule for K (my call; the builder may tune it with measurements):** for each tile, take the largest K for which each axis's rounding adds at most about a quarter of the tile. Also, K is no more than log2 of the cell's shorter side, and no more than 3 when the padded image sits in the shared atlas, whose pages keep four levels. The shader caps each tile's level at its own K, so K travels with the tile, not the tileset.

Under this rule:

| Tiles | Cells | K | Size of the cells against the tiles |
|---|---|---|---|
| Power-of-two (16, 32, 256) | the tile itself | down to one texel a tile | none |
| 256 x 352 | 256 x 384 | 7 | 1.09x |
| 120 x 140 | 128 x 160 | 5 | 1.22x |
| 129 | 160 | 5 | 1.54x (the worst kind) |

Extrusion stays as it is: each tile's edge texels fill the rest of its cell, so a coarse texel at a tile's edge averages only that tile's colours.

### 2. The tile shader picks its level and keeps the read inside the cell at that level

Every draw from a padded image uses tile fragment stages of its own (the same three variants as now: straight, premultiplied, linear-page). That covers tile chunks, y-sorted and per-cell items, and tile objects. Each fragment:
1. Takes the UV derivatives where control flow is uniform, as now.
2. Computes the level L from them, in the page's texels (the isotropic level, log2 of the longer derivative), and caps it at the tile's K. An L within a small tolerance of a whole number is snapped to it, so zooms of exactly 1, 1/2 or 1/4 read one level, not a sliver of the next.
3. Reads the two levels L lies between, each with its own explicit-level sample (`textureSampleLevel`) and its UV clamped into the cell, inset by half a texel of that level. It blends the two by L's fraction. A whole L reads one level. (My call. One trilinear sample inset for the coarser level would also blur every tile's outer texel at the finer level, softening the edges of tiles zoomed out only slightly, and making them jump as a zoom passes through 1. Two samples keep the finer level's edge exact.)

Outside the cell, the fragment stays transparent, as `sample_masked` is now.

At zoom 1 and above the level is 0. One sample then reads the full-size level, inset by half a texel, with the magnification filter, as now.

Below level K a tile is drawn at most a few pixels wide. It keeps sampling level K, which can shimmer slightly as the camera moves but never shows a neighbour.

On the pixel-art target the nearest sampler keeps level 0, as now. The shader's level must agree with that (level 0 and its inset), or it must leave the sampler's clamp to decide while insetting for level 0.

Standalone textures keep their full mip chain. The levels past K go unread, and they add less than a third of level K's size. Trimming the chain to K + 1 levels is optional (builder's call).

### 3. What stays as it is

- Padding off the landing frame, the shared `PaddedCache`, and the 120-frame keep window.
- How an embedded tileset is keyed.
- Animations repeated to a common beat (`MAX_ANIMATION_CELLS`). Their frames step by their cell's width and height, which may now differ.
- Flips and hex turns.
- The odd-size isometric raster offset and tile offsets.
- The "too big to pad" refusal against the device's largest texture side. It now trips far later.
- The partial-upload rule on Apple GPUs: padded textures are still written whole.

### 4. The demo

- **Sketch Town returns to full size (approved).** The `tile-grids` demo uses Kenney's Sketch Town pictures at their own size on the town and staggered-island pages, which share its tilesets. The cameras zoom out to keep today's views: the town's zooms become a quarter and a half. This shows smaller copies drawn clean on a real page.
- **The Hexagon Pack returns to full size too (my call)**, since it was halved only for memory: 120 x 140 hexagons, cells doubled, and the board's camera zoomed to match.
- **`scripts/derive-art.sh`** keeps only the cuts that aren't for memory: the hex board's chosen hexagons, and Sketch Town's empty top rows if the builder keeps that cut.
- **`make-grid-maps` rewrites the maps** through Tiled's headless `--evaluate`. Tiled's window is never opened.
- **The README** records the art and the new memory figures. Its section on why the pictures are halved goes.

### 5. Docs

- **`docs/backlog/tilemap.md`:** rewrite the big-tile Limit. The new memory rule replaces it, plus a Limit for the shimmer below level K. Record the measured memory and padding times.
- **The `ryo-tilemap` skill:** replace "Keep big art small" and the "too big to pad" figures with the new rule, and bump the skill's minor version once.
- **Module notes:** update the tilemap and render notes (the `padded.rs` entry, Tile chunks, Texture slots if the bindings change).
- **`docs/backlog/follow-ups.md`:** remove the big-tile padding lines this work fixes.

## How we know it works

- **The layout:** unit tests on cell sizes, K and margins.
  - Tiles of 16, 32, 129, 256, 256 x 352 and 120 x 140 px.
  - An image collection of mixed sizes (each picture in a cell of its own size and alignment, no two cells overlapping), and animations whose frames differ in size.
  - Both homes: the atlas, and a texture of its own.
  - Every cell starts on a multiple of 2^K, and the texture's sides are multiples of 2^K.
- **No bleeding** (`src/core/testing/tile_bleeding.rs`, extended). Tilesets whose neighbouring tiles have colours never drawn, drawn at zooms from 4 down past each tileset's floor, at sub-pixel camera offsets, on both targets.
  - Big non-power-of-two tiles in a texture of their own, small tiles in the atlas, and an image collection of mixed sizes.
  - Flipped, hex-turned and animated tiles, a tile object and a y-sorted layer.
  - Where a tile is wider than a few pixels, no pixel shows a neighbour's colour.
  - The existing seam tests keep their purpose and their assertions on pixels. Their tile counts may rise: the "own texture" cases add tiles so the padded image passes 256 px, and under the new layout they would fit the atlas instead (`no_seams_at_any_zoom_or_subpixel_camera`). Hard-coded margins in the GPU tests (`gpu_world.rs`) follow the new layout.
- **Sharp edges when zoomed out a little:** a test reads a tile with a distinct outer texel ring at zooms between 1/2 and 1, and checks the ring still shows in proportion, not blurred into the next texel. The bleed test can't catch softened edges, because it only looks for neighbours' colours.
- **The failing test comes first.** The new bleed test is written and shown to fail against the new layout with the old sampling. Its scenes stay small (a few tiles, a 64 px frame), so no run allocates more than a few hundred MB.
- **Memory:** a test pads the demo's full-size tilesets and checks their bytes with smaller copies: at most 30 MB together, and none above twice its decoded art. Another checks a tileset of a few big power-of-two tiles (four of 256 px) stays at its art plus its smaller copies. A third checks the castle-and-flowers collection stays within 2.5 times its art.
- **No visible change:** the scripted tour comparison against main.
  - Every page that loads no tileset must be byte-identical.
  - Pages with tiles may differ only by rounding (my call). Every tile moves to new texel coordinates in a texture of a new size, and sprites sharing an atlas page with a padded image may move slots. The float error that remains can turn a filtered sample by one step of a colour channel, or flip a nearest-sampled pixel whose centre falls exactly on a texel boundary at a fractional zoom.
  - Every differing pixel is looked at, and the report says what each difference is. Anything visible blocks the merge. The demo pages this work changes on purpose are compared by eye.
- **Speed:** the tilemap guards keep their held targets. The record in `docs/backlog/tilemap.md` gains the padding time and memory per demo tileset, before and after.

## Constraints

- Work on a branch in the main checkout, never a worktree.
- LF line endings.
- The gate: `cargo fmt`, `clippy -D warnings`, the wasm32 check of `ryo2d`, `cargo check -p skill-examples`, and `cargo test --workspace`.
- Every Rust example in a skill must compile.
- No stage or task labels in user-facing text.
- Never copy Tiled's GPL source. Tiled runs headless only.
- Breaking API changes are fine, though none reaches a game: `PaddedLayout` is public but hidden from the docs, and its one `cell` gives way to each tile's own cell.

## Left to the builder

- The exact rule for K, if measurements argue for another threshold than a quarter.
- How each tile's K reaches the shader: an instance lane for chunks and tile objects, or another per-tile route.
- How cells of mixed sizes are packed.
- How tile objects reach the tile fragment stages: a variant of the sprite pipeline, or drawing them through the tile path.
- Whether standalone padded textures trim their mip chain to K + 1 levels.
- How the pixel-art target's level agrees with the nearest sampler's clamp.
