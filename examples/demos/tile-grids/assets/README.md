# Tile grids demo assets

The art is Kenney's, CC0. The Tiled project, tilesets and maps are written by `make-grid-maps`, through Tiled itself; nothing here is drawn or placed by hand. The ground streamed in beyond the meadow, and the endless world of the third page, are made in code as they stream (below, "The generated worlds"), as are the speed page's maps and their tiles (below, "The speed page").

## Art

| Pack | Version | Source | Downloaded | Licence | Files used |
|---|---|---|---|---|---|
| Pixel Platformer by Kenney | 1.2 (07-10-2023) | https://kenney.nl/assets/pixel-platformer | from the tilemap demo (below) | CC0 1.0, `kenney-pixel-platformer/License.txt` | `Tilemap/tilemap_packed.png` (18 px tiles, 20 x 9): the meadow and the ground generated beyond it, the painted ground, the coins, and the endless world (its earth, tinted dark, is the backdrop where the ground is dug or hollow) |
| Tiny Dungeon by Kenney | 1.0 (05-07-2022) | https://kenney.nl/assets/tiny-dungeon | from the tilemap demo (below) | CC0 1.0, `kenney-tiny-dungeon/License.txt` | `Tilemap/tilemap_packed.png` (16 px tiles, 12 x 11): the runner (tile 88, an adventurer), as on the tilemap demo's side-view pages, and the digger (tile 87, a dwarf in a horned helm); the town's knight (97), the island's walker (99) and the oblique dungeon's knight (96); the dungeon's grey brick walls (40) and sand floors (48, 49); and the speed page's tiles (below) |
| Sketch Town by Kenney | 1.0 (12-05-2021) | https://kenney.nl/assets/sketch-town | 2026-11-04 from Kenney's upload to OpenGameArt, https://opengameart.org/content/sketch-town: `kenney_sketchtown_1.0.zip`, sha256 `bd8bb09cb33c922347c643242a04de5ea14206511efe1be5cd7b975a2563b77e` | CC0 1.0, `kenney-sketch-town/License.txt` | 25 pictures of `Tiles/` (256 x 352 each, drawn for isometric cells of 232 x 110): `grass_center_N`, `water_center_N`, `grass_water_N/E/S/W`, `grass_waterConcave_N/E/S/W` and `grass_waterConvex_N/E/S/W` (the ground), `castle_wall_N/E`, `castle_tower_N`, `castle_gateOpen_N/E`, `tree_single_N/E`, `tree_multiple_N/E` and `rocks_grass_N/E` (the props) |
| Hexagon Pack by Kenney | 1.0 (2015) | https://kenney.nl/assets/hexagon-pack | 2026-11-04 from Kenney's upload to OpenGameArt, https://opengameart.org/content/hexagon-pack-310x: `hexagon-pack.zip`, sha256 `e4847bf2142e1d808b59bd3039579d21dcce3a871370659598e9cb9ffad1c92c` | CC0 1.0, `kenney-hexagon-pack/License.txt` | `Spritesheets/hexagonTerrain_sheet.png` (1024 x 2048: pointy-topped hexagons of 120 x 140, 2 px apart, 8 a row; its right column's few smaller pictures are not used) |

The Pixel Platformer and Tiny Dungeon sheets and their licences are copied unchanged from the tilemap demo (`examples/demos/tilemap/assets/`), whose README records each pack's download (the zip, its date and its sha256). kenney.nl's own download could not be fetched from the command line for the two new packs, so their zips are the ones Kenney uploaded to OpenGameArt, the same packs under the same licence; each zip's `License.txt` says CC0 and is copied beside its pictures with its line endings made LF. Only the files the pages use are copied, unchanged. Credit is optional under CC0; Kenney is credited here all the same.

- **Sketch Town is a collection tileset**, one picture a tile: the pack has no sheet. At this scale (25 pictures) that is a draw per picture a chunk, which costs nothing to notice.
- **The Hexagon Pack's terrain sheet is an atlas tileset**, so the board draws from one texture.
- **The terrain painted on the town** is the Wang set `terrain` of `tilesets/sketch-town-ground.tsx`: a corner set of grass (1) and water (2) over the pack's water edges, which cover every corner combination but the two diagonals (painting one warns once and leaves its cell as it was).
- **Hex turns are shown on the sand** (tile 26, `sand_07`), whose dune ripples are flat top-down art that reads correctly turned: the board turns each sand hex by a hash of its cell, a sixth of a turn at a time. Nothing whose picture stands up (the trees, rocks and cacti) is turned.
- **Big pictures take a moment to land.** The engine draws each tileset from a padded copy that gives every tile a power-of-two cell of its own (a Sketch Town picture 512 x 512, a hexagon 256 x 256), made off the main thread: about 0.2 to 0.4 s for the town, the island and the board, so those pages show their maps a moment after they open (and a headless run stepping as fast as it can, such as the tour's capture at frame 90, can outpace it).

The ball on the Paint anywhere page is drawn in code (`src/paint.rs`, `ball_image`).

## Files

| File | Written by | What it holds |
|---|---|---|
| `project.tiled-project` | Tiled (the script adds the types) | Enum `Facing`; classes `player_start` (`facing`) and `coin` (`value` 1, for objects and tiles) |
| `tilesets/platformer.tsx` | Tiled | Pixel Platformer: the terrain tiles solid (0-5, 20-25, 104, 120-123, 140-143), the rope bridge (90-92) and the planks (146, 147) one-way, the coin (151) of class `coin` and animated, the water's surface (33) animated |
| `tilesets/sketch-town-ground.tsx` | Tiled | Sketch Town's 14 ground pictures, offset (-12, 116): the blocks drawn a cell lower than the pack's own tileset draws them, so their tops lie on the cells. Water (1) collides, its shape the cell's diamond in the picture's pixels; the Wang set `terrain` (above). The tileset's grid is orthogonal |
| `tilesets/sketch-town-props.tsx` | Tiled | Sketch Town's 11 props, offset (-12, 6) (the pack's own), so a prop stands on its cell, on the block's top: walls (0, 1) and the tower (2) collide, their shape the cell's diamond; the open gates (3, 4), trees (5-8) and rocks (9, 10) do not. `_N` walls run along the map's x, `_E` along its y; a `_N` gate's arch faces down and right, an `_E` gate's down and left |
| `tilesets/hexagon-terrain.tsx` | Tiled | The Hexagon Pack's terrain sheet: 120 x 140, spacing 2, 8 columns, 112 tiles; nothing collides |
| `tilesets/dungeon.tsx` | Tiled | Tiny Dungeon's sheet, 16 px: the grey brick wall (40) solid, a rectangle filling the tile |
| `maps/endless-meadow.tmx` | Tiled | An infinite map of 18 px cells, saved as CSV in Tiled's chunks of 16 x 16 cells. `scenery` (11 chunks; trees, pines, sprouts, mushrooms, bushes, signs and clouds, not colliding), `water` (1 chunk; the pond, not colliding), `ground` (18 chunks; the meadow from cell -96 to cell 63 with its grass at row 0 and its bottom at row 5, two hills, a plateau, a pond under the rope bridge, both bank to bank, a dip, planks, and an island of 9 x 3 cells at cell (-204, -40)), and `objects`: a `player_start` facing left at cell (-16, 0), and 23 tile objects of the coin tile with no class of their own, which take the tile's `coin` |
| `maps/town.tmx` | Tiled | Isometric, 16 x 16 cells of 232 x 110. `ground`: grass blocks, and a pond right of the castle whose water lies at corner points (13 to 15, 1 to 4) and (12, 2) and (12, 3), its edges picked by the Wang set's corners. `props` (y-sorted): a castle wall round cells 2 to 10 each way, a tower at each corner and an open gate at (10, 6) and (6, 10), and trees and rocks by a hash of the cell, outside and inside, never beside the walls. `objects`: the `player_start` at cell (8, 6), by the gate. Neither tile layer casts shadows |
| `maps/island-rows.tmx`, `maps/island-columns.tmx` | Tiled | The same island on two staggered grids of 232 x 110 cells, each 2900 x 1375 px: rows shifted (along y, odd rows half a cell right; 12 x 24 cells) and columns shifted (along x, odd columns half a cell down; 24 x 12). `ground`: grass where a cell's centre is inside the island's outline (an ellipse with a wavy edge, in pixels, so both grids show one island), else water, which collides. `props` (y-sorted): groves where a value noise of the centre is high, and a few trees and rocks by a hash of the cell, none in the middle. `objects`: the `player_start` in the middle |
| `maps/hex-board.tmx` | Tiled | Hexagonal, 8 x 9 pointy-topped hexagons of 120 x 140 with sides of 70, in rows, odd rows half a hexagon right. `board`: grass, pines and trees, rocks, dirt, stone, sand (turned) and cacti, from the script's `BOARD` rows. Above it four empty layers the page marks, each with an int property `mark` naming its rim tile: `range` (sand, 26, see-through at 0.45), `ring` (the beige rim, 74), `neighbours` (the grey rim, 33) and `line` (the red rim, 66). No layer collides or casts shadows |
| `maps/dungeon.tmx`, `maps/dungeon-mirror.tmx` | Tiled | Oblique, 24 x 16 cells of 16 px, skewed 4 px across a row, and -4 in the mirror. `floor`: sand by a hash of the cell; `walls` (colliding): round the edge, columns at x 8 (rows 3 to 10) and 16 (rows 5 to 12), a short row at y 8 and four pillars. `objects`: the `player_start` at cell (3, 7) |

The engine reads the meadow into chunks of its own, 32 x 32 cells: 11 of the 27 that the map's bounds, the cells holding tiles (cells (-204, -41) to (63, 5)), reach. Streamed, those 11 are the hand-made part of an endless world: a stream never generates a chunk the file touches, the cells Tiled left empty included, so the meadow ends on the edges of its chunks (cells -96 and 63 are the first and last of chunks -3 and 1), and the generated ground meets it there.

### How the files are made

`make-grid-maps` (`src/bin/make-grid-maps.rs`) writes an empty `project.tiled-project` and runs `scripts/make-maps.js` with Tiled's `--evaluate`. The script adds the project's types, builds the tilesets and the maps through Tiled's scripting API (`map.infinite = true`; `map.orientation`, `staggerAxis`, `staggerIndex`, `hexSideLength` and `skewX` for the other grids; tiles written at any cell through a layer's `edit()`, with the hex turns' flags), and saves each with Tiled's own writers, which lay an infinite map's layers out in chunks. A map takes its tilesets from scratch maps that name their files, so it refers to them rather than embedding them. Every choice is made by rule or by a hash of the cell, so a run writes the same bytes each time. The ground's tiles are picked by the four cells beside each one (a grass top where the cell above is open, an edge on each open side, a bottom edge where the cell below is open), the rule the Paint anywhere page paints by and the generators lay by (`src/shared.rs`, `ground_tile`). Past each end of the meadow the script counts the ground as going on, at that end's grass row down to the bottom row, as the generated ground does, so the end columns have no edge facing it and the seam reads as one world.

`make-grid-maps` then opens and exports each file with Tiled's command line (`--export-tileset json`, `--export-map json`, with the project), which fails on anything Tiled cannot read, checks the meadow's export is an infinite map with chunks at negative cells on both axes, and checks each other map's export has the grid it was written with (its orientation, stagger axis, hex side or skew).

## Regenerating

```sh
cargo run -p tile-grids-demo --bin make-grid-maps
```

It needs Tiled 1.12.2 (`$TILED`, `/Applications/Tiled.app`, or `tiled` on the path); without it nothing is written. Tiled runs headless (`--evaluate` and the export flags): its window never opens. `cargo test -p tile-grids-demo` loads the maps through the engine and plays the pages.

## The generated worlds

Two pages stream chunks from a generator (`src/terrain.rs`), a pure function of the chunk run on the engine's chunk builder thread, so a chunk that unloads and comes back comes back the same:

- **The meadow's world** (page 1, `MeadowWorld`): the meadow's ground going on past both its ends, read from the file (its bottom row, its end columns and their grass rows), with the same tileset and the same `ground_tile` rule, its gentle hills rising from the seam over about 18 columns, and the meadow's sprouts, pines, mushrooms, bushes, trees and clouds. Chunks above the clouds and below the bottom row come back empty.
- **The endless page's world** (page 3, `Wilds`): rolling hills of earth going down without end, caves opening six cells or more under the grass, sprouts, pines and clouds, and a backdrop layer of earth tinted dark behind everything under the grass.

The heights and the caves are `ryo2d-procgen`'s noise: `generate_heightmap` (Perlin with octaves), its offset set to the chunk's first cell, so a chunk samples the noise at its own cells and chunks side by side meet. Heights are read along one row of the 2D noise (row 11.3), so a column's height is the noise at that column. Where scenery stands is a hash of the column.

## The speed page

Speed on every grid (page 8, `src/speed.rs`) builds the speed guards' map in code on the grid chosen: 1024 x 1024 cells in four layers, a floor on every cell (Tiny Dungeon's sand and flagstones, 48, 49 and 42), cave rock where value noise is high (its grey bricks, 40 and 28, colliding), props on about one open cell in 16 (a chest, tombstones, an anvil, a table and a stool: 89, 64, 65, 74, 72, 73) and small things on one in 64 (potions and blades: 113 to 116, 103, 104). Its tileset is an atlas made from Tiny Dungeon's sheet as the page opens, each tile cut to the grid's cells: as it is on the orthogonal and oblique grids (16 px), masked to the hexagon on the hexagonal grid (16 x 16, sides of 8), and laid flat on the diamond on the isometric and staggered grids (32 x 16).

## Editors

- Tiled 1.12.2, from Homebrew (`brew install --cask tiled`), `/Applications/Tiled.app`.

## Hand-play: open and save each file in Tiled

The files were written by script; open and save each in Tiled so anything the editor would write differently shows up in `git diff` before the merge.

1. Open `project.tiled-project` (File > Open Project...). Open and save `tilesets/platformer.tsx`, then `maps/endless-meadow.tmx`.
   - Look at: the map is infinite (Map > Map Properties), its chunks reach left of and above the origin, the island sits far up and left, the coins take their class and `value` from the coin tile, and the bridge's and planks' collision shapes are one-way (Tile Collision Editor).
2. Open and save `tilesets/sketch-town-ground.tsx`, `sketch-town-props.tsx`, `hexagon-terrain.tsx` and `dungeon.tsx`, then `maps/town.tmx`, `island-rows.tmx`, `island-columns.tmx`, `hex-board.tmx`, `dungeon.tmx` and `dungeon-mirror.tmx`.
   - Look at: the town's walls and trees stand on the blocks' tops and the blocks' tops on the cells; the water's and the walls' collision shapes lie on the cell's diamond (Tile Collision Editor); the `terrain` Wang set (Terrain Sets panel: paint and erase a few cells of the town's ground with the terrain brush, then undo); the two islands are one island; the board's sand hexes are turned (their ripples run three ways) and its mark layers are empty, `range` see-through; the dungeon leans right and its mirror left.
   - In Tiled's collision editor, does a tileset whose grid is isometric show its collision shapes projected onto the diamond, or in the image's pixels? Switch `sketch-town-props.tsx`'s orientation to isometric (Tileset Properties) to see, and don't save that change. The engine reads them in the image's pixels, and warns once per file for such a tileset.
3. Run `cargo test -p tile-grids-demo` and look over `git diff`.
