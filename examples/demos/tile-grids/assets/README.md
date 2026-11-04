# Tile grids demo assets

The art is Kenney's, CC0; the Sketch Town and Hexagon Pack pictures are halved from the packs' by `scripts/derive-art.sh` (below). The Tiled project, tilesets and maps are written by `make-grid-maps`, through Tiled itself; nothing here is drawn or placed by hand. The ground streamed in beyond the meadow, and the endless world of the third page, are made in code as they stream (below, "The generated worlds"), as are the speed page's maps and their tiles (below, "The speed page").

## Art

| Pack | Version | Source | Downloaded | Licence | Files used |
|---|---|---|---|---|---|
| Pixel Platformer by Kenney | 1.2 (07-10-2023) | https://kenney.nl/assets/pixel-platformer | from the tilemap demo (below) | CC0 1.0, `kenney-pixel-platformer/License.txt` | `Tilemap/tilemap_packed.png` (18 px tiles, 20 x 9): the meadow and the ground generated beyond it, the painted ground, the coins, and the endless world (its earth, tinted dark, is the backdrop where the ground is dug or hollow) |
| Tiny Dungeon by Kenney | 1.0 (05-07-2022) | https://kenney.nl/assets/tiny-dungeon | from the tilemap demo (below) | CC0 1.0, `kenney-tiny-dungeon/License.txt` | `Tilemap/tilemap_packed.png` (16 px tiles, 12 x 11): the runner (tile 88, an adventurer), as on the tilemap demo's side-view pages, and the digger (tile 87, a dwarf in a horned helm); the town's knight (97), the island's walker (99) and the oblique dungeon's knight (96); the dungeon's grey brick walls (40) and sand floors (48, 49); and the speed page's tiles (below) |
| Sketch Town by Kenney | 1.0 (12-05-2021) | https://kenney.nl/assets/sketch-town | 2026-11-04 from Kenney's upload to OpenGameArt, https://opengameart.org/content/sketch-town: `kenney_sketchtown_1.0.zip`, sha256 `bd8bb09cb33c922347c643242a04de5ea14206511efe1be5cd7b975a2563b77e` | CC0 1.0, `kenney-sketch-town/License.txt` | 25 pictures of `Tiles/` (256 x 352 each, drawn for isometric cells of 232 x 110), halved: `grass_center_N`, `water_center_N`, `grass_water_N/E/S/W`, `grass_waterConcave_N/E/S/W` and `grass_waterConvex_N/E/S/W` (the ground), `castle_wall_N/E`, `castle_tower_N`, `castle_gateOpen_N/E`, `tree_single_N/E`, `tree_multiple_N/E` and `rocks_grass_N/E` (the props) |
| Hexagon Pack by Kenney | 1.0, released 2015 (from its kenney.nl page; the zip's `License.txt` names no version), uploaded to OpenGameArt 2015-10-28 | https://kenney.nl/assets/hexagon-pack | 2026-11-04 from Kenney's upload to OpenGameArt, https://opengameart.org/content/hexagon-pack-310x: `hexagon-pack.zip`, sha256 `e4847bf2142e1d808b59bd3039579d21dcce3a871370659598e9cb9ffad1c92c` | CC0 1.0, `kenney-hexagon-pack/License.txt` | 24 of the 120 x 140 pointy-topped hexagons of `Spritesheets/hexagonTerrain_sheet.png` (positions from its `hexagonTerrain_sheet.xml`), halved into `kenney-hexagon-pack/hexagon-board.png` |

The Pixel Platformer and Tiny Dungeon sheets and their licences are copied unchanged from the tilemap demo (`examples/demos/tilemap/assets/`), whose README records each pack's download (the zip, its date and its sha256). kenney.nl's own download could not be fetched from the command line for the two new packs, so their zips are the ones Kenney uploaded to OpenGameArt, the same packs under the same licence; each zip's `License.txt` says CC0 and is copied beside its pictures with its line endings made LF. Credit is optional under CC0; Kenney is credited here all the same.

**The Sketch Town and Hexagon Pack pictures are not the packs' files but derived from them** (CC0 as they are), by `scripts/derive-art.sh` with ImageMagick 7.1.2 (a box filter: each texel the mean of the four it halves), because the engine pads big tiles into big cells (below):
- each Sketch Town picture used, halved from 256 x 352 to 128 x 176 and cut to its bottom 128 rows (the 48 above are empty in every one of them): 128 x 128, for isometric cells of 116 x 55;
- the 24 hexagons the board uses, cut from the terrain sheet where its XML says they are, halved from 120 x 140 to 60 x 70 and laid out six a row in `hexagon-board.png` (360 x 280; the order is the script's, and `HEX_SHEET` in `scripts/make-maps.js`): `grass_05`, `grass_10`, `grass_12`, `grass_13`, `grass_11`, `grass_14`; `grass_15`, `grass_16`, `dirt_06`, `dirt_11`, `dirt_12`, `dirt_14`; `dirt_13`, `dirt_16`, `dirt_18`, `dirt_17`, `stone_07`, `sand_07`; `sand_14`, `sand_13`, `sand_12`, `sand_01`, `stone_01`, `mars_01`.

The board draws at zoom 1, a texel a pixel, where the full-size art drew at a half, so it looks as it did; the town and the island draw at zoom 1/2 by default, as the full-size art did at 1/4.

- **Sketch Town is a collection tileset**, one picture a tile: the pack has no sheet. At this scale (25 pictures) that is a draw per picture a chunk, which costs nothing to notice.
- **The board's hexagons are an atlas tileset**, `hexagon-board.png`, so the board draws from one texture.
- **The terrain painted on the town** is the Wang set `terrain` of `tilesets/sketch-town-ground.tsx`: a corner set of grass (1) and water (2) over the pack's water edges, which cover every corner combination but the two diagonals (painting one warns once and leaves its cell as it was).
- **Hex turns are shown on the sand** (`sand_07`), whose dune ripples are flat top-down art that reads correctly turned: the board turns each sand hex by a hash of its cell, a sixth of a turn at a time. Nothing whose picture stands up (the trees, rocks and cacti) is turned.
- **Why the pictures are halved: the padding.** The engine draws each tileset from a padded copy in which every tile gets a power-of-two cell of side npot(T + max(T, 8)) texels, T the tileset's largest tile's longer side, with a cell's margin round the image and every mip level (a third more): about four times a tile's texels, up to sixteen. It is made off the main thread natively, and within the landing frame on the web. At the packs' own size a 352 px Sketch Town picture took a 1,024 px cell and a 140 px hexagon a 512 px one: the ground tileset padded to 6,144 x 6,144 (192 MiB with its mips), the props to 6,144 x 5,120 (160 MiB) and the terrain sheet's 112 hexagons to 6,656 x 6,656 (225 MiB), and the pages landed 0.2 to 0.4 s after they opened. Halved and cut, a picture's cell is 256 px: the ground pads to 1,536 x 1,536 (12 MiB), the props to 1,536 x 1,280 (10 MiB) and the board's 24 hexagons to 1,792 x 1,792 (16 MiB), and each page lands in about 20 ms.

The ball on the Paint anywhere page is drawn in code (`src/paint.rs`, `ball_image`).

## Files

| File | Written by | What it holds |
|---|---|---|
| `project.tiled-project` | Tiled (the script adds the types) | Enum `Facing`; classes `player_start` (`facing`) and `coin` (`value` 1, for objects and tiles) |
| `tilesets/platformer.tsx` | Tiled | Pixel Platformer: the terrain tiles solid (0-5, 20-25, 104, 120-123, 140-143), the rope bridge (90-92) and the planks (146, 147) one-way, the coin (151) of class `coin` and animated, the water's surface (33) animated |
| `tilesets/sketch-town-ground.tsx` | Tiled | Sketch Town's 14 ground pictures (128 x 128), offset (-6, 58): the blocks drawn a cell lower than the pack's own tileset draws them (its offset (-12, 6), halved, is (-6, 3)), so their tops lie on the cells. Water (1) collides, its shape the cell's diamond in the picture's pixels (`64,15 122,42.5 64,70 6,42.5`); the Wang set `terrain` (above). The tileset's grid is orthogonal |
| `tilesets/sketch-town-props.tsx` | Tiled | Sketch Town's 11 props (128 x 128), offset (-6, 3), so a prop stands on its cell, on the block's top: walls (0, 1) and the tower (2) collide, their shape the cell's diamond (`64,70 122,97.5 64,125 6,97.5`); the open gates (3, 4), trees (5-8) and rocks (9, 10) do not. `_N` walls run along the map's x, `_E` along its y; a `_N` gate's arch faces down and right, an `_E` gate's down and left |
| `tilesets/hexagon-board.tsx` | Tiled | `hexagon-board.png`: 60 x 70, 6 columns, 24 tiles; nothing collides |
| `tilesets/dungeon.tsx` | Tiled | Tiny Dungeon's sheet, 16 px: the grey brick wall (40) solid, a rectangle filling the tile |
| `maps/endless-meadow.tmx` | Tiled | An infinite map of 18 px cells, saved as CSV in Tiled's chunks of 16 x 16 cells. `scenery` (11 chunks; trees, pines, sprouts, mushrooms, bushes, signs and clouds, not colliding), `water` (1 chunk; the pond, not colliding), `ground` (18 chunks; the meadow from cell -96 to cell 63 with its grass at row 0 and its bottom at row 5, two hills, a plateau, a pond under the rope bridge, both bank to bank, a dip, planks, and an island of 9 x 3 cells at cell (-204, -40)), and `objects`: a `player_start` facing left at cell (-16, 0), and 23 tile objects of the coin tile with no class of their own, which take the tile's `coin` |
| `maps/town.tmx` | Tiled | Isometric, 16 x 16 cells of 116 x 55 (the art's diamond, 55 tall, so half-cell steps fall on half pixels, which the engine places as Tiled does). `ground`: grass blocks, and a pond right of the castle whose water lies at corner points (13 to 15, 1 to 4) and (12, 2) and (12, 3), its edges picked by the Wang set's corners. `props` (y-sorted): a castle wall round cells 2 to 10 each way, a tower at each corner and an open gate at (10, 6) and (6, 10), and trees and rocks by a hash of the cell, outside and inside, never beside the walls. `objects`: the `player_start` at cell (8, 6), by the gate. Neither tile layer casts shadows |
| `maps/island-rows.tmx`, `maps/island-columns.tmx` | Tiled | The same island on two staggered grids of 116 x 55 cells, each 1,450 x 675 px (676 with the tiles' overhang): rows shifted (along y, odd rows half a cell right; 12 x 24 cells) and columns shifted (along x, odd columns half a cell down; 24 x 12). Tiled steps staggered grids in whole pixels, half a cell rounding down: rows 27 px apart and shifted 58 along y; columns 58 apart, their cells 54 apart and shifted 27, along x. The cells' diamonds are 116 x 54, and each 55 px tile stands on its diamond's bottom, reaching a pixel above it (the script's `centre()` steps as Tiled does). `ground`: grass where a cell's centre is inside the island's outline (an ellipse with a wavy edge, in pixels, so both grids show one island), else water, which collides. `props` (y-sorted): groves where a value noise of the centre is high, and a few trees and rocks by a hash of the cell, none in the middle. `objects`: the `player_start` in the middle |
| `maps/hex-board.tmx` | Tiled | Hexagonal, 8 x 9 pointy-topped hexagons of 60 x 70 with sides of 35, in rows, odd rows half a hexagon right. `board`: grass, pines and trees, rocks, dirt, stone, sand (turned) and cacti, from the script's `BOARD` rows. Above it four empty layers the page marks, each with an int property `mark` naming its rim tile: `range` (`sand_07`, 17, see-through at 0.45), `ring` (the beige rim `sand_01`, 21), `neighbours` (the grey rim `stone_01`, 22) and `line` (the red rim `mars_01`, 23). No layer collides or casts shadows |
| `maps/dungeon.tmx`, `maps/dungeon-mirror.tmx` | Tiled | Oblique, 24 x 16 cells of 16 px, skewed 4 px across a row, and -4 in the mirror. `floor`: sand by a hash of the cell; `walls` (colliding): round the edge, columns at x 8 (rows 3 to 10) and 16 (rows 5 to 12), a short row at y 8 and four pillars. `objects`: the `player_start` at cell (3, 7) |

The engine reads the meadow into chunks of its own, 32 x 32 cells: 11 of the 27 that the map's bounds, the cells holding tiles (cells (-204, -41) to (63, 5)), reach. Streamed, those 11 are the hand-made part of an endless world: a stream never generates a chunk the file touches, the cells Tiled left empty included, so the meadow ends on the edges of its chunks (cells -96 and 63 are the first and last of chunks -3 and 1), and the generated ground meets it there.

### How the files are made

`make-grid-maps` (`src/bin/make-grid-maps.rs`) writes an empty `project.tiled-project` and runs `scripts/make-maps.js` with Tiled's `--evaluate`. The script adds the project's types, builds the tilesets and the maps through Tiled's scripting API (`map.infinite = true`; `map.orientation`, `staggerAxis`, `staggerIndex`, `hexSideLength` and `skewX` for the other grids; tiles written at any cell through a layer's `edit()`, with the hex turns' flags), and saves each with Tiled's own writers, which lay an infinite map's layers out in chunks. A map takes its tilesets from scratch maps that name their files, so it refers to them rather than embedding them. Every choice is made by rule or by a hash of the cell, so a run writes the same bytes each time. The ground's tiles are picked by the four cells beside each one (a grass top where the cell above is open, an edge on each open side, a bottom edge where the cell below is open), the rule the Paint anywhere page paints by and the generators lay by (`src/shared.rs`, `ground_tile`). Past each end of the meadow the script counts the ground as going on, at that end's grass row down to the bottom row, as the generated ground does, so the end columns have no edge facing it and the seam reads as one world.

`make-grid-maps` then opens and exports each file with Tiled's command line (`--export-tileset json`, `--export-map json`, with the project), which fails on anything Tiled cannot read, checks the meadow's export is an infinite map with chunks at negative cells on both axes, and checks each other map's export has the grid it was written with (its orientation, stagger axis, hex side or skew).

## Regenerating

```sh
cargo run -p tile-grids-demo --bin make-grid-maps
```

It needs Tiled 1.12.2 (`$TILED`, `/Applications/Tiled.app`, or `tiled` on the path); without it nothing is written. Tiled runs headless (`--evaluate` and the export flags): its window never opens. It prints `TilesetManager: There are still 5 tilesets loaded at exit!`: Tiled's own note on the scratch maps `external()` reads to refer to the tilesets by file, harmless. `cargo test -p tile-grids-demo` loads the maps through the engine and plays the pages.

The halved Sketch Town and Hexagon Pack pictures come from the packs' zips (above), each unzipped in a folder of its own, with ImageMagick 7:

```sh
scripts/derive-art.sh <sketch town>/Tiles <hexagon pack>/Spritesheets
```

run from `examples/demos/tile-grids`. A run writes the same bytes each time with the same ImageMagick.

## The generated worlds

Two pages stream chunks from a generator (`src/terrain.rs`), a pure function of the chunk run on the engine's chunk builder thread, so a chunk that unloads and comes back comes back the same:

- **The meadow's world** (page 1, `MeadowWorld`): the meadow's ground going on past both its ends, read from the file (its bottom row, its end columns and their grass rows), with the same tileset and the same `ground_tile` rule, its gentle hills rising from the seam over about 18 columns, and the meadow's sprouts, pines, mushrooms, bushes, trees and clouds. Chunks above the clouds and below the bottom row come back empty.
- **The endless page's world** (page 3, `Wilds`): rolling hills of earth going down without end, caves opening six cells or more under the grass, sprouts, pines and clouds, and a backdrop layer of earth tinted dark behind everything under the grass.

The heights and the caves are `ryo2d-procgen`'s noise: `generate_heightmap` (Perlin with octaves), its offset set to the chunk's first cell, so a chunk samples the noise at its own cells and chunks side by side meet. Heights are read along one row of the 2D noise (row 11.3), so a column's height is the noise at that column. Where scenery stands is a hash of the column.

## The speed page

Speed on every grid (page 8, `src/speed.rs`) builds the speed guards' map in code on the grid chosen: 1024 x 1024 cells in four layers, a floor on every cell (Tiny Dungeon's sand and flagstones, 48, 49 and 42), cave rock where value noise is high (its grey bricks, 40 and 28, colliding), props on about one open cell in 16 (a chest, tombstones, an anvil, a table and a stool: 89, 64, 65, 74, 72, 73) and small things on one in 64 (potions and blades: 113 to 116, 103, 104). Its million cells are made on the main thread as the page opens and when the Grid setting changes (a hitch in that step); the engine then works out its chunks, colliders and shadows off the main thread, and the map lands about 55 to 100 ms later. Its tileset is an atlas made from Tiny Dungeon's sheet as the page opens, each tile cut to the grid's cells: as it is on the orthogonal and oblique grids (16 px), masked to the hexagon on the hexagonal grid (16 x 16, sides of 8), and laid flat on the diamond on the isometric and staggered grids (32 x 16).

## Editors

- Tiled 1.12.2, from Homebrew (`brew install --cask tiled`), `/Applications/Tiled.app`.

## Hand-play: open and save each file in Tiled

The files were written by script; open and save each in Tiled so anything the editor would write differently shows up in `git diff` before the merge.

1. Open `project.tiled-project` (File > Open Project...). Open and save `tilesets/platformer.tsx`, then `maps/endless-meadow.tmx`.
   - Look at: the map is infinite (Map > Map Properties), its chunks reach left of and above the origin, the island sits far up and left, the coins take their class and `value` from the coin tile, and the bridge's and planks' collision shapes are one-way (Tile Collision Editor).
2. Open and save `tilesets/sketch-town-ground.tsx`, `sketch-town-props.tsx`, `hexagon-board.tsx` and `dungeon.tsx`, then `maps/town.tmx`, `island-rows.tmx`, `island-columns.tmx`, `hex-board.tmx`, `dungeon.tmx` and `dungeon-mirror.tmx`.
   - Look at: the town's walls and trees stand on the blocks' tops and the blocks' tops on the cells; the water's and the walls' collision shapes lie on the cell's diamond (Tile Collision Editor); the `terrain` Wang set (Terrain Sets panel: paint and erase a few cells of the town's ground with the terrain brush, then undo); the two islands are one island; the board's sand hexes are turned (their ripples run three ways) and its mark layers are empty, `range` see-through; the dungeon leans right and its mirror left.
   - In Tiled's collision editor, does a tileset whose grid is isometric show its collision shapes projected onto the diamond, or in the image's pixels? Switch `sketch-town-props.tsx`'s orientation to isometric (Tileset Properties) to see, and don't save that change. The engine reads them in the image's pixels, and warns once per file for such a tileset.
3. In the demo (`cargo run -p tile-grids-demo`), on the town page, paint water on two cells that share only a corner: the Wang set has no tile for that diagonal, so the cell between keeps its tile and the dev log warns once (an incomplete set); that warning is expected. Painting water never reaches the cells round the knight.
4. Run `cargo test -p tile-grids-demo` and look over `git diff`.
