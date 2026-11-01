# Tile grids demo assets

The art is Kenney's, CC0. The Tiled project, tileset and map are written by `make-grid-maps`, through Tiled itself; nothing here is drawn or placed by hand. The ground streamed in beyond the meadow, and the endless world of the third page, are made in code as they stream (below, "The generated worlds").

## Art

| Pack | Version | Source | Licence | Files used |
|---|---|---|---|---|
| Pixel Platformer by Kenney | 1.2 (07-10-2023) | https://kenney.nl/assets/pixel-platformer | CC0 1.0, `kenney-pixel-platformer/License.txt` | `Tilemap/tilemap_packed.png` (18 px tiles, 20 x 9): the meadow and the ground generated beyond it, the painted ground, the coins, and the endless world (its earth, tinted dark, is the backdrop where the ground is dug or hollow) |
| Tiny Dungeon by Kenney | 1.0 (05-07-2022) | https://kenney.nl/assets/tiny-dungeon | CC0 1.0, `kenney-tiny-dungeon/License.txt` | `Tilemap/tilemap_packed.png` (16 px tiles, 12 x 11): the runner (tile 88, an adventurer), as on the tilemap demo's side-view pages, and the digger (tile 87, a dwarf in a horned helm) |

Both sheets and their licences are copied unchanged from the tilemap demo (`examples/demos/tilemap/assets/`), whose README records each pack's download (the zip, its date and its sha256). Credit is optional under CC0; Kenney is credited here all the same.

The ball on the Paint anywhere page is drawn in code (`src/paint.rs`, `ball_image`).

## Files

| File | Written by | What it holds |
|---|---|---|
| `project.tiled-project` | Tiled (the script adds the types) | Enum `Facing`; classes `player_start` (`facing`) and `coin` (`value` 1, for objects and tiles) |
| `tilesets/platformer.tsx` | Tiled | Pixel Platformer: the terrain tiles solid (0-5, 20-25, 104, 120-123, 140-143), the rope bridge (90-92) and the planks (146, 147) one-way, the coin (151) of class `coin` and animated, the water's surface (33) animated |
| `maps/endless-meadow.tmx` | Tiled | An infinite map of 18 px cells, saved as CSV in Tiled's chunks of 16 x 16 cells. `scenery` (11 chunks; trees, pines, sprouts, mushrooms, bushes, signs and clouds, not colliding), `water` (1 chunk; the pond, not colliding), `ground` (18 chunks; the meadow from cell -96 to cell 63 with its grass at row 0 and its bottom at row 5, two hills, a plateau, a pond under the rope bridge, both bank to bank, a dip, planks, and an island of 9 x 3 cells at cell (-204, -40)), and `objects`: a `player_start` facing left at cell (-16, 0), and 23 tile objects of the coin tile with no class of their own, which take the tile's `coin` |

The engine reads the meadow into chunks of its own, 32 x 32 cells: 11 of the 27 that the map's bounds, the cells holding tiles (cells (-204, -41) to (63, 5)), reach. Streamed, those 11 are the hand-made part of an endless world: a stream never generates a chunk the file touches, the cells Tiled left empty included, so the meadow ends on the edges of its chunks (cells -96 and 63 are the first and last of chunks -3 and 1), and the generated ground meets it there.

### How the files are made

`make-grid-maps` (`src/bin/make-grid-maps.rs`) writes an empty `project.tiled-project` and runs `scripts/make-maps.js` with Tiled's `--evaluate`. The script adds the project's types, builds the tileset and the map through Tiled's scripting API (`map.infinite = true`; tiles written at any cell through a layer's `edit()`), and saves each with Tiled's own writers, which lay an infinite map's layers out in chunks. Every choice is made by rule or by a hash of the cell, so a run writes the same bytes each time. The ground's tiles are picked by the four cells beside each one (a grass top where the cell above is open, an edge on each open side, a bottom edge where the cell below is open), the rule the Paint anywhere page paints by and the generators lay by (`src/shared.rs`, `ground_tile`). Past each end of the meadow the script counts the ground as going on, at that end's grass row down to the bottom row, as the generated ground does, so the end columns have no edge facing it and the seam reads as one world.

`make-grid-maps` then opens and exports each file with Tiled's command line (`--export-tileset json`, `--export-map json`, with the project), which fails on anything Tiled cannot read, and checks the meadow's export is an infinite map with chunks at negative cells on both axes.

## Regenerating

```sh
cargo run -p tile-grids-demo --bin make-grid-maps
```

It needs Tiled 1.12.2 (`$TILED`, `/Applications/Tiled.app`, or `tiled` on the path); without it nothing is written. `cargo test -p tile-grids-demo` loads the meadow through the engine and plays the pages.

## The generated worlds

Two pages stream chunks from a generator (`src/terrain.rs`), a pure function of the chunk run on the engine's chunk builder thread, so a chunk that unloads and comes back comes back the same:

- **The meadow's world** (page 1, `MeadowWorld`): the meadow's ground going on past both its ends, read from the file (its bottom row, its end columns and their grass rows), with the same tileset and the same `ground_tile` rule, its gentle hills rising from the seam over about 18 columns, and the meadow's sprouts, pines, mushrooms, bushes, trees and clouds. Chunks above the clouds and below the bottom row come back empty.
- **The endless page's world** (page 3, `Wilds`): rolling hills of earth going down without end, caves opening six cells or more under the grass, sprouts, pines and clouds, and a backdrop layer of earth tinted dark behind everything under the grass.

The heights and the caves are `ryo2d-procgen`'s noise: `generate_heightmap` (Perlin with octaves), its offset set to the chunk's first cell, so a chunk samples the noise at its own cells and chunks side by side meet. Heights are read along a row off the noise's lattice (row 11.3), where every octave would be zero. Where scenery stands is a hash of the column.

## Editors

- Tiled 1.12.2, from Homebrew (`brew install --cask tiled`), `/Applications/Tiled.app`.

## Hand-play: open and save each file in Tiled

The files were written by script; open and save each in Tiled so anything the editor would write differently shows up in `git diff` before the merge.

1. Open `project.tiled-project` (File > Open Project...). Open and save `tilesets/platformer.tsx`, then `maps/endless-meadow.tmx`.
   - Look at: the map is infinite (Map > Map Properties), its chunks reach left of and above the origin, the island sits far up and left, the coins take their class and `value` from the coin tile, and the bridge's and planks' collision shapes are one-way (Tile Collision Editor).
2. Run `cargo test -p tile-grids-demo` and look over `git diff`.
