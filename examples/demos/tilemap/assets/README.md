# Tilemap demo assets

The art is Kenney's, CC0. The maps, tilesets, Tiled project, world and LDtk project are written by `make-maps`; nothing here is drawn or placed by hand.

## Art

| Pack | Version | Source | Downloaded | Licence | Files used |
|---|---|---|---|---|---|
| Pixel Platformer by Kenney | 1.2 (07-10-2023) | https://kenney.nl/assets/pixel-platformer | 2026-10-30, `kenney_pixel-platformer.zip`, sha256 `d01a196dbe3cc964e00d83ba3b987df62f332dc9260c9f941b4fbcc9047130f4` | CC0 1.0, `kenney-pixel-platformer/License.txt` | `Tilemap/tilemap_packed.png` (18 px tiles, 20 x 9), `Tilemap/tilemap-backgrounds_packed.png` (24 px) |
| Tiny Dungeon by Kenney | 1.0 (05-07-2022) | https://kenney.nl/assets/tiny-dungeon | 2026-10-30, `kenney_tiny-dungeon.zip`, sha256 `c109438ab06f65fd80f9b2686a4cf9c7c11dc64444b47333ec71d602f8bb5fc7` | CC0 1.0, `kenney-tiny-dungeon/License.txt` | `Tilemap/tilemap_packed.png` (16 px tiles, 12 x 11) |

The sheets are copied unchanged (each pack's `License.txt` with its line endings made LF). Credit is optional under CC0; Kenney is credited here all the same.

`images/` holds tiles `make-maps` derives from those sheets, pixel by pixel (`src/bin/make-maps/art.rs`), for what the packs do not draw:

- `ground.png` (18 px, 5 x 4): Pixel Platformer's terrain tiles for a complete corner Wang set (top-left, top, top-right, the four inner corners, left, fill, right, bottom-left, bottom, bottom-right), the two diagonal tiles (an inner corner with the opposite one's notch added), a second fill, two 45° slopes (the top edge's outline and grass laid along the diagonal over the fill) and the tiles under them (the slope's grass carried into the inner corner).
- `dungeon-terrain.png` (16 px, 8 x 5): Tiny Dungeon's walls and floors for a complete wall and floor corner Wang set (the diagonals made the same way), rock and floor variants, walls one cell thick (beams, ends and a lone block, made from halves and quarters of the corner tiles), the steps, two slopes (a wall's rim along the diagonal) with their feet, and three wall tops with the floor between their top stones taken out, for walls seen from the side.
- `backdrop.png`: Pixel Platformer's green background (sky, trees and ground), cut from the backgrounds sheet.

## Files

| File | Written by | What it holds |
|---|---|---|
| `project.tiled-project` | Tiled (the script adds the types) | Enum `Facing`; classes `player_start` (`facing`), `coin` (`value` 1), `switch` (`on` false), `door` (`open` false, `switch`: an object reference) and `spikes` (`damage` 1) |
| `tilesets/platformer.tsx` | Tiled | Pixel Platformer: terrain and blocks solid, rope bridge (90-92) and planks (146, 147) one-way, pine trunk solid, coin, water and waterfall animated, class tiles `coin` (151) and `spikes` (68) |
| `tilesets/ground.tsx` | Tiled | `images/ground.png`: Wang corner set `ground` (colour `ground`), every corner combination; slopes 16 (`/`) and 17 (`\`) as three-corner polygons |
| `tilesets/dungeon.tsj` | Tiled | Tiny Dungeon: walls solid, statues 64 and 65 solid at the base with an `occluder` shape for the body, gargoyle (19) and drain (43) animated |
| `tilesets/dungeon-terrain.tsj` | Tiled | `images/dungeon-terrain.png`: Wang corner set `dungeon`, colours `wall` (1) and `floor` (2), every combination; floor is the higher, so procgen's `to_terrain_map` keeps corridors one cell wide; steps one-way, slopes 31 and 32 |
| `maps/platformer.tmx` | Tiled | 48 x 16 of 18 px: an image layer (`backdrop`, repeated, parallax 0.5), `scenery` and `water` (not colliding), `ground` (both tilesets, written base64 zlib; the rest CSV), `doors`, `hazards` (spikes), `objects`: a `player_start`, 13 tile objects of the coin tile, with no class of their own: they take the tile's `coin` (one worth 10), a `switch` (the lever) and a `door` whose `switch` property refers to it |
| `maps/topdown.tmj` | Tiled | 30 x 20 of 16 px: `backdrop` image layer, sand `paths`, `trees` (y-sorted: pines and statues), `clouds` (parallax 1.25, not colliding, occluding or lit), a `player_start` |
| `rooms/rooms.world` | make-maps, as Tiled writes it | Three rooms side by side |
| `rooms/room-1.tmj`, `room-2.tmx`, `room-3.tmj` | Tiled | 20 x 12 of 16 px: `terrain` (the dungeon Wang set, occluding), brick `faces` under the north wall, y-sorted `props` that cast no shadow, `objects` (room 1's `player_start`); doorways where the rooms meet |
| `ldtk/dungeon.ldtk` | make-maps, as LDtk 1.5.3 writes it | Levels `Platformer` (the character demo's `PLATFORMER_LEVEL`, 30 x 30 of 16 px, with its player, pickups and checkpoints) and `Vault` to its right (chests, a door referring to its switch, a sign, a patrol path, torches); an IntGrid `Collisions` layer (`solid`, `one_way`, `slope_bottom_right`, `slope_bottom_left`) whose auto-layer rules draw `images/dungeon-terrain.png` |

Tiled's files are written by Tiled itself: `make-maps` runs `scripts/make-maps.js` with Tiled's `--evaluate`, and the script builds each tileset and map through Tiled's scripting API and saves it with Tiled's own writers. Tiled saves every layer of a map in one format, so the script saves the platformer twice (CSV and base64 zlib) and puts the zlib ground layer into the CSV file. `make-maps` then opens and exports every Tiled file with Tiled's command line (`--export-tileset` / `--export-map json`, with the project), which fails on anything Tiled cannot read. LDtk has no command line: its project follows LDtk's JSON schema 1.5.3 and its sample projects, and the auto-layer's tiles saved in it are the ones its rules give, worked out the way LDtk applies rules (first match wins, cells outside the level count as solid).

## Regenerating

```sh
cargo run -p tilemap-demo --bin make-maps
```

It needs Tiled 1.12.2 (`$TILED`, `/Applications/Tiled.app`, or `tiled` on the path) for the Tiled files; without it, the images, world and LDtk project are written and the Tiled files left alone. A run writes the same bytes each time. `cargo test -p tilemap-demo` loads and spawns every file through the engine.

## Editors

- Tiled 1.12.2, from Homebrew (`brew install --cask tiled`), `/Applications/Tiled.app`.
- LDtk 1.5.3, from its GitHub release (`v1.5.3`, `mac-distribution.zip`, its dmg), installed to `/Applications/LDtk.app` on 2026-10-30.

## Hand-play: open and save each file in its editor

The files were written by script; open and save each in its editor so anything the editor would write differently shows up in `git diff` before the merge.

1. Tiled: open `project.tiled-project` (File > Open Project...). Open and save `tilesets/platformer.tsx`, `ground.tsx`, `dungeon.tsj` and `dungeon-terrain.tsj`, then `maps/platformer.tmx`, `maps/topdown.tmj` and the three rooms. Load `rooms/rooms.world` as a world and check the rooms join at their doorways.
   - Look at: the ground's Wang set (Terrain Sets panel, paint and erase a few cells with the terrain brush), the platformer's door (its `switch` property points at the lever), coins taking their class and `value` from the coin tile, the slopes' and bridge's collision shapes (Tile Collision Editor).
   - Expected rewrite: saving `maps/platformer.tmx` writes its `ground` layer as CSV, since Tiled keeps one layer format per map. Run `make-maps` again (or discard that change) to keep the zlib layer.
2. LDtk: open `ldtk/dungeon.ldtk`. Check both levels draw their auto-layer tiles and the entities show their fields, then save. LDtk works out the auto-layer again on saving; expect the same tiles (maybe listed in another order) and fields LDtk fills in (`cachedPixelData`, `__smartColor`).
3. Run `cargo test -p tilemap-demo` and look over `git diff`.
