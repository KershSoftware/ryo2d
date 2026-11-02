> The sample maps and scripts this note quotes (`tiled/`) were made in a scratch folder during the 6b research (2026-10-31) and are not committed; the 6b fixtures replace them.

# Tiled 1.12.2 map formats and drawing: recon for Ryo2D

Sources: Tiled's reference docs (json-map-format, tmx-map-format, global-tile-ids, worlds, terrain pages, 1.12 release notes, GitHub issue #2824), files saved by Tiled 1.12.2 itself, and measurements from Tiled's scripting API (`TileMap.tileToScreen`/`pixelToScreen`/`tileToPixel`, and `TileMap.toImage()` renders that I measured pixel by pixel). No Tiled source was read.

Everything lives beside this file in `tiled/`:

- `tiled/gen.js`: writes every sample map to `tiled/maps/*.tmj` and `*.tmx`. Run `Tiled --evaluate gen.js`; it uses absolute paths.
- `tiled/measure.js`, `order*.js`, `wang.js`, `probe.js`, `rt.js`: the measurement scripts quoted below.
- `tiled/mkpng.py`: generates the tiny PNG tilesets in `tiled/images/`.
- `tiled/maps/`: samples `iso_*`, `stag_{x,y}_{odd,even}`, `hex_{x,y}_{odd,even}`, `hex_*_rot120`, `oblique_*`, `inf_*`, `ortho_ro0-3`, `wang_hex.tsj/.tsx`, `sample.world`.
- `tiled/tilesets/`: a hand-written iso-grid tileset and its Tiled round trip.
- `tiled/render/`: a few `toImage()` renders.

Caveats, so nothing here is over-trusted:

- Maps were built by scripts. Script-made tilesets are embedded and have `grid width="0" height="0"`; a GUI-made iso tileset has a real grid.
- The GUI itself was not driven. Anything that depends on a GUI-only path is marked "docs only" or "unverified".
- Headless `tiled.loadWorld()` returned `undefined`, so `.world` behaviour is docs only.

## Enum values seen through the scripting API (not file values)

| Name | Values |
|---|---|
| `TileMap.Orthogonal/Isometric/Staggered/Hexagonal/Oblique` | 1, 2, 3, 4, 5 |
| `StaggerX/StaggerY` | 0, 1 |
| `StaggerOdd/StaggerEven` | 0, 1 |
| `RightDown/RightUp/LeftDown/LeftUp` | 0, 1, 2, 3 |
| `layerDataFormat` XML/Base64/Base64Gzip/Base64Zlib/Base64Zstandard/CSV | 0..5 |
| `Tile` flag bits, scripting side | FlipH 1, FlipV 2, AntiDiagonal 4, RotatedHexagonal120 8 |

A new `TileMap()` defaults to Base64Zlib.

## 1. Orientation-related fields

### Map

| Field (JSON and TMX share names) | Type | Meaning, and when it is written |
|---|---|---|
| `orientation` | string | `orthogonal`, `isometric`, `staggered`, `hexagonal`, `oblique`. |
| `staggeraxis` | `"x"`/`"y"` | Only written for `staggered` and `hexagonal`. `y` means rows are shifted (hex: pointy-top). `x` means columns are shifted (hex: flat-top). |
| `staggerindex` | `"odd"`/`"even"` | Only for `staggered` and `hexagonal`. Which rows (axis `y`) or columns (axis `x`) are shifted by half a cell. Indices are 0-based: `odd` shifts indices 1, 3, 5...; `even` shifts 0, 2, 4.... |
| `hexsidelength` | int | Only for `hexagonal`, always written (a `hexsidelength: 0` map writes `0`). It is the length in pixels of the hex side parallel to the stagger axis. With stagger y it is the vertical side, with stagger x the horizontal one. |
| `skewx` | int | Only for `oblique`, and only written when non-zero. Screen pixels each tile row shifts X by. Negative values are written, e.g. `skewx="-10"`. |
| `skewy` | int | Same, for the Y shift per tile column. |
| `renderorder` | string | `right-down`, `right-up`, `left-down`, `left-up`. Written for every orientation (an isometric map with `left-up` saves `"renderorder":"left-up"`), but Tiled only honours it on orthogonal maps (section 2). |
| `infinite` | bool (JSON) / `0`/`1` (TMX) | True means layer data is in chunks. |
| `width`, `height` | int | Map size in tiles. **For an infinite map both are 0** in both formats. |
| `tilewidth`, `tileheight` | int | The cell: iso diamond bounding box, staggered cell box, hex tile box, ortho cell. For hex it is the full bounding box of the hex (so `tilewidth` 32 and `hexsidelength` 12 means a pointy hex 32 wide, 12 tall on the vertical side). |
| `compressionlevel` | int | Default -1. |
| `editorsettings.chunksize` | `{width,height}` | **The "output chunk size".** It is only written when it differs from 16x16. There are no `outputchunkwidth`/`outputchunkheight` fields in 1.12.2: I grepped every sample, and the docs do not list them. TMX: `<editorsettings><chunksize width="8" height="4"/></editorsettings>` as the first child of `<map>`. The 1.12 notes added `TileMap.chunkSize` and `compressionLevel` to the scripting API. |

The oblique parameters are **`skewx` and `skewy`**: pixel offsets per tile row and per tile column.

Excerpts:

```
<map ... orientation="oblique" renderorder="right-down" width="4" height="4" tilewidth="32" tileheight="32" infinite="0" skewx="8" skewy="12" ...>
<map ... orientation="hexagonal" ... infinite="0" hexsidelength="12" staggeraxis="y" staggerindex="odd" ...>
{"orientation":"staggered","renderorder":"right-down","staggeraxis":"y","staggerindex":"odd","tilewidth":64,"tileheight":32,...}
```

### Tile layer (infinite)

JSON infinite tile layer: `startx`, `starty` (the bounding box origin in tiles; here -32,-16), `width`, `height` (the bounds: 64, 48 here), `chunks`, and layer-level `encoding` and `compression`. In TMX, the `<layer>` of a script-made infinite layer had `width="0" height="0"`. Unlike JSON it did not carry the bounds, so the engine should derive bounds from the chunks and ignore layer width/height when `infinite`.

### Tileset

| Field | JSON | TMX | Meaning |
|---|---|---|---|
| `tileoffset` | `{"x":3,"y":-5}` | `<tileoffset x="3" y="-5"/>` | Pixels added to the drawn image, +y down. Applies to layer tiles and tile objects (measured). Omitted when 0,0. |
| `objectalignment` | string | attribute | `unspecified` (default, omitted), `topleft`, `top`, `topright`, `left`, `center`, `right`, `bottomleft`, `bottom`, `bottomright`. Anchor of tile *objects* only. Unspecified means bottom-left, except isometric maps where it means `bottom` (measured, section 2). |
| `tilerendersize` | `"tile"`/`"grid"` | attribute | `grid` draws tiles at the *map's* `tilewidth`x`tileheight` (measured: a tileset grid of 32x16 did not change it). Omitted when `tile`. |
| `fillmode` | `"stretch"`/`"preserve-aspect-fit"` | attribute | Only matters with `tilerendersize="grid"` (measured). |
| `grid` | `{"orientation":"isometric","width":64,"height":32}` | `<grid orientation="isometric" width="64" height="32"/>` | Tileset grid orientation (`orthogonal` default or `isometric`) and cell size. It only drives the editor's terrain/collision overlays (docs and measurement: it changed no placement). Always written by 1.12.2, even as `orthogonal 0x0`. |
| `transformations` | `{"hflip":true,"vflip":true,"rotate":true,"preferuntransformed":true}` | `<transformations hflip="1" vflip="1" rotate="1" preferuntransformed="0"/>` | Which transforms Wang/terrain painting may use. It does not stop flags appearing in a layer's gids. |

Excerpt (`iso_tileoffset_grid.tmx`):

```
<tileset firstgid="1" name="tiles" tilewidth="64" tileheight="64" tilecount="4" columns="4" tilerendersize="grid" fillmode="preserve-aspect-fit">
 <tileoffset x="3" y="-5"/>
 <grid orientation="orthogonal" width="0" height="0"/>
```

### GID flags (Global Tile IDs page, and confirmed in saved files)

| Mask | Orthogonal, isometric, staggered | Hexagonal |
|---|---|---|
| `0x80000000` | flip horizontally | flip horizontally |
| `0x40000000` | flip vertically | flip vertically |
| `0x20000000` | anti-diagonal flip (rotation with H/V) | **rotate 60 degrees clockwise** |
| `0x10000000` | ignored | **rotate 120 degrees clockwise** |

The docs say to clear all four bits (`& ~0xF0000000`) even on non-hex maps, because Tiled keeps the 0x10000000 bit when a map's orientation changes.

## 2. How Tiled places and draws tiles

Measured with `toImage()`, using solid-colour tiles and bounding-box scans (`measure.js`). Notation: "cell box" is the cell's `tilewidth` x `tileheight` box. All maps had `tileToScreen` for the anchor.

### Layer tiles: one rule everywhere

**The tile image's bottom-left corner sits on the bottom-left corner of the cell box.** It is not centred, so a wider image extends right and a taller image extends up. This holds for orthogonal, isometric, staggered and hexagonal maps.

| Orientation | `tileToScreen(cell)` returns | Image placement (measured) |
|---|---|---|
| Orthogonal | cell top-left | image bottom-left = cell bottom-left. A 64x64 tile in a 32x32 cell at (1,2) covers x 32..96, y 32..96. |
| Isometric 64x32 | the diamond's **top vertex** (x = centre, y = top of the box) | A 64x64 image at (1,2): top vertex (96,48), image x 64..128, y 16..80. Its bottom edge is the diamond's bottom vertex row (top + 32), horizontally centred when width equals tilewidth. A 96-wide image: x 64..160 (left aligned, not centred). A 32x32 image: x 64..96. |
| Staggered | cell box top-left, already shifted | Same bottom-left rule against the shifted box. x-stagger odd, cell (1,2): box top-left (32,80), image 64x64 at x 32..96, y 48..112. |
| Hexagonal | cell box top-left | A 32x32 image at pointy (1,2): box (32,44), image at x 32..64, y 44..76. A 48x48 image: x 32..80, y 28..76 (anchored bottom-left). |
| Oblique | the skewed top-left of the cell | Image bottom-left sits on the skewed screen position of tile corner (tx, ty+1) = `pixelToScreen(tx*tw, (ty+1)*th)`. The image itself is not sheared. With skewx 16, a 64x64 image at (1,2) covers x 80..144, y 32..96 (80 = 32 + 3*16). |

- **Tileoffset** is added after that anchoring (+3,-5 moves the image by 3 right and 5 up).
- **`tilerendersize="grid"`** scales the image to the map's cell size (stretch fills the whole cell box; `preserve-aspect-fit` centres horizontally and bottom-aligns). Example: a 48x48 image in a 64x32 iso cell fits to 32x32 at x 80..112.

### Cell positions (this is the map-space geometry)

Let `tw`, `th` be the tile size, `(c, r)` the cell, `W`, `H` the map size in tiles.

- **Isometric:** `x = (c - r) * tw/2 + ox`, `y = (c + r) * th/2`. `ox = H * tw/2` for a fixed-size map (measured on 6x3: ox = 96, bounds 288x144), and `ox = 0` for an infinite map (cell (-2,-1) lands at x = -32). Bounds are `(W+H)*tw/2` by `(W+H)*th/2`.
- **Staggered, axis y:** `x = c*tw + (shifted(r) ? tw/2 : 0)`, `y = r*th/2`. Bounds: `W*tw + tw/2` by `(H+1)*th/2`. **Axis x:** `x = c*tw/2`, `y = r*th + (shifted(c) ? th/2 : 0)`. Bounds: `(W+1)*tw/2` by `H*th + th/2`. `shifted(i)` is `i odd` for `staggerindex="odd"` and `i even` for `even` (0-based). Verified: stag y odd (2,1) -> (160,16) = 2*64 + 32, 1*16. Even -> (128,16).
- **Hexagonal, axis y (pointy):** `x = c*tw + (shifted(r) ? tw/2 : 0)`, `y = r * (th + side)/2`. Verified with tw=th=32, side=12: row step 22, (2,1) odd -> (80,22), even -> (64,22). Bounds: `W*tw + tw/2` by `(H-1)*(th+side)/2 + th`. **Axis x (flat):** `x = c * (tw + side)/2`, `y = r*th + (shifted(c) ? th/2 : 0)`: (2,1) odd -> (44,32), even -> (44,48). Bounds `(W-1)*(tw+side)/2 + tw` by `H*th + th/2`. With `hexsidelength` 0 this collapses to the staggered formulas.
- **Oblique:** `screen = (px + py/th*skewx, py + px/tw*skewy)` where `(px, py)` is the unskewed pixel position. Checked at (80,48) with 32x32 cells, skewx 8, skewy 12: -> (92,78). Bounds: `W*tw + |H*skewx|` by `H*th + |W*skewy|`. A negative skew moves the content to negative x (screen x of (0,32) is -10), so the engine must take the minimum into account.

On odd sizes Tiled steps staggered and hexagonal grids in whole pixels and rounds a bounded isometric map's origin down, and a hexagon's side may be longer than its tile: section 7.

### Tile objects (anchor)

The object's `x,y` is the anchor, and the image is placed relative to it. Measured on a 64x64 image, iso 64x32 map, object at pixel (64,96), which maps to screen (96,80):

| `objectalignment` | Result |
|---|---|
| unspecified | **bottom-centre**: image x 64..128, y 16..80 |
| `bottomleft` | x 96..160, y 16..80 |
| `topleft` | x 96..160, y 80..144 |
| `center` | x 64..128, y 48..112 |

Orthogonal, staggered, hex and oblique maps use **bottom-left** by default (ortho: 64x64 at (32,96) -> x 32..96, y 32..96; oblique: image bottom-left on the screen point; hex and staggered likewise). The object's own `width`/`height` size the image. `tileoffset` shifts tile objects too (measured on iso: +16 y moved the box +16).

### Draw order inside a layer

**Orthogonal** honours all four `renderorder`s. All are "row outer, column inner": the last-drawn tile wins on overlap.

| renderorder | Rows | Columns within a row |
|---|---|---|
| `right-down` | top to bottom | left to right |
| `right-up` | bottom to top | left to right |
| `left-down` | top to bottom | right to left |
| `left-up` | bottom to top | right to left |

Measured with overlapping 48x48 tiles in 32x32 cells: with `right-down`, (2,3) covers (3,2); with `right-up`, (3,2) covers (2,3).

**Isometric, staggered, hexagonal (and oblique):** `renderorder` is ignored. I saved all four values and compared overlapping renders: they were identical. The order is always `right-down`:

- Rows top to bottom, columns left to right (iso, hex y, stagger y, with `staggerindex` irrelevant to order).
- **Axis-x staggered and hex maps are not column-major.** They are row-major, but within a row the half-cell-lowered (shifted) columns draw on top of the unshifted neighbours. Measured: stagger x odd, (3,2) over (2,2); stagger x even (shifted = even columns), (2,2) over (3,2) and (4,2) over (3,2). This matches painter order by the cell's screen y. Hex x gave the same.
- The oblique render order was not separately measured (it comes after the ortho path in my gen, and I did not test overlap there). Measured since (section 7): oblique maps draw as orthogonal ones, honouring the render order.

## 3. Object positions on each orientation

Object `x`/`y` are doubles in a map-pixel space; polygon points are relative to the object's x,y. I verified with `pixelToScreen`/`screenToPixel`/`pixelToTile` and by round-tripping saved samples.

- **Orthogonal, staggered, hexagonal:** the space is plain screen pixels, no projection. `pixelToScreen` is the identity (stag/hex samples). The tile-to-pixel top-left helper `tileToPixel(c,r)` equals `tileToScreen(c,r)` (cell box top-left, stagger included).
- **Isometric:** the space is the *unprojected* grid scaled by **`tileheight` on both axes**: pixel = tile coordinate * `tileheight` (even when tilewidth is not twice tileheight; a 48x32 map also gave (64,32) for tile coord (2,1)). Fractional tile coordinates are allowed. Projection:
  `screen.x = (px - py)/th * tw/2 + ox`, `screen.y = (px + py)/2`.
  A tile coordinate of (2,1) is the *corner* shared by cells, not the cell centre: it is the top vertex of cell (2,1). A tile object that should stand on cell (c,r) (bottom-centre anchor) needs its anchor on the cell's bottom vertex, i.e. tile coordinate (c+1, r+1) * th.
- **Oblique:** the space is the unskewed orthogonal pixel grid (`px = tile*tw`, `py = tile*th`); screen = the skew formula above.

Worked examples (samples built with `gen.js`; cell (2,1); the tile coordinate is passed through `tileToPixel`):

| Sample | tile coordinate | saved `x,y` (JSON and TMX) | screen point |
|---|---|---|---|
| `iso_2to1` (64x32, 4x4) | (2,1) | **64, 32** | (160, 48), top vertex of cell (2,1) |
| `iso_2to1`, the point object at tile (2.5,1.5) | (2.5,1.5) | **80, 48** | (160, 64), centre of the cell |
| `iso_3to2` (48x32, 4x4) | (2,1) | **64, 32** | (120, 48) |
| `stag_y_odd` (64x32) | cell (2,1) | **160, 16** (row 1 is shifted) | same |
| `stag_y_even` | cell (2,1) | **128, 16** | same |
| `stag_x_odd` | cell (2,1) | **64, 32** | same |
| `stag_x_even` | cell (2,1) | **64, 48** (column 2 is shifted) | same |
| `hex_y_odd` (32x32, side 12) | cell (2,1) | **80, 22** | same |
| `hex_y_even` | cell (2,1) | **64, 22** | same |
| `hex_x_odd` | cell (2,1) | **44, 32** | same |
| `hex_x_even` | cell (2,1) | **44, 48** | same |
| `oblique_b` (32x32, skewx 8, skewy 12) | (2,1) | **64, 32** | (72, 56) |
| `inf_iso` | (-2,-1) | **-64, -32** | (-32, -48) |
| `inf_hex_y_odd` | cell (-2,-1) | **-48, -22** | same |

For staggered and hex, the helper returns the cell box top-left, so a centred point object on a cell needs `+ (tw/2, th/2)` added by the author; the saved numbers are just whatever was typed. Excerpt (isometric JSON):

```
{"id":4,"name":"tileobj","gid":1,"x":64,"y":32,"width":64,"height":64,...}
{"id":2,"name":"point","point":true,"x":80,"y":48,...}
{"id":3,"name":"poly","polygon":[{"x":0,"y":0},{"x":16,"y":0},{"x":16,"y":16},{"x":0,"y":8}],"x":64,"y":32,...}
```

Polygon points are in the same space as x,y (on iso that means unprojected units, not screen pixels). Objects in 1.12.2 JSON carry `"type":""` for class; there is no `class` key on empty objects.

## 4. Infinite maps

Seen in `inf_ortho_*.tmj/.tmx` and `inf_iso`, `inf_stag_y_odd`, `inf_hex_y_odd`, `inf_oblique`. **Tiled allows infinite for every orientation**, all written the same way. A tile layer is written with only the non-empty chunks, which come out sorted by chunk y then chunk x.

- Default chunk size is 16x16 (nothing stored). Chunks are aligned to multiples of the chunk size (floor), so tiles at -17 land in the chunk starting at -32. With 8x4 chunks the origins were (-16,-16), (8,-16), (-8,-4), (-24,0), (0,0), (16,20).
- **Empty chunks are omitted.** Tiles inside a chunk are still full chunk-width/height arrays, with 0 for empty. In XML-plain, an empty cell is `<tile/>`.
- JSON chunk: `{"x":-16,"y":-16,"width":16,"height":16,"data":[...]}` (x,y in tiles; `data` is an array for CSV, a string for base64).
- TMX: `<data encoding=... compression=...>` wraps one or more `<chunk x y width height>`; chunks carry no encoding or compression of their own.
- The tile at (-5,-3) became index 219 = 13*16 + 11 in chunk (-16,-16), as expected for row-major.

Encodings in chunks (identical in a fixed-size layer, which has `data` directly instead):

| Format | JSON (layer-level keys) | TMX `<data>` | Chunk payload |
|---|---|---|---|
| CSV | no `encoding`/`compression` keys; chunk `data` is an int array | `encoding="csv"` | rows separated by newlines, each row ending with a comma except the last |
| XML plain | (JSON has no such mode; writing it gives CSV) | no attributes; `<tile gid="1"/>`, `<tile/>` children | the deprecated per-tile form (1273 `<tile/>` lines in my 16x16 file) |
| base64 | `"encoding":"base64","compression":""` | `encoding="base64"` | little-endian u32 array, 1024 bytes for 16x16 |
| zlib | `"compression":"zlib"` | `compression="zlib"` | base64 of a zlib stream (starts `eJx`) |
| gzip | `"compression":"gzip"` | `compression="gzip"` | base64 of gzip (starts `H4sI`) |
| zstd | `"compression":"zstd"` | `compression="zstd"` | base64 of a zstd frame (starts `KLUv/`, magic `28 b5 2f fd`) |

I decoded base64, zlib and gzip with Python (256 values; the zstd frame has the standard magic but I had no zstd decoder). The gids come out exactly as set. Decoding applies per chunk, with the same flag bits in the high 4 bits of each u32.

Top-level of an infinite map: `"infinite":true,"width":0,"height":0`. The tile layer, JSON excerpt: `"startx":-32,"starty":-16,"width":64,"height":48` with chunks at x,y -16,-16 / 0,-16 / -32,0 / 0,0 / 16,16. In my TMX, `<layer ... width="0" height="0">`.

Objects on infinite maps can have negative coordinates (see section 3). In an infinite iso map the screen origin is 0 (no `H*tw/2` offset), so the engine needs a constant origin that is independent of the map size.

### World files (docs only)

A `.world` is JSON: `{"type":"world","onlyShowAdjacentMaps":bool,"maps":[{"fileName","x","y","width","height"}], "patterns":[{"regexp","multiplierX","multiplierY","offsetX","offsetY","mapWidth","mapHeight"}]}`. All numbers are pixels. A pattern's first capture group becomes x and the second y, each times its multiplier, plus the offset. Width/height are needed for adjacency mode. The docs say nothing about infinite maps or non-orthogonal ones in worlds; `maps/sample.world` mixes an infinite map and an iso map but headless `loadWorld` returned nothing, so how Tiled anchors an infinite map is unverified. The natural reading is that the world x,y is where the map's tile origin (0,0) sits, with the chunks extending to negative offsets, and an infinite map's width/height need to be supplied explicitly (a saved infinite map has `width=0`).

## 5. Terrain (Wang) brush

- Wang sets are stored in the tileset, with no knowledge of orientation. Type is `corner`, `edge` or `mixed`. Each tile gets an 8-entry `wangid`: top, top-right, right, bottom-right, bottom, bottom-left, left, top-left; 0 = unset, colours start at 1. Excerpt: `<wangtile tileid="0" wangid="1,1,1,1,1,1,1,1"/>`; JSON `"wangid":[1,1,1,1,1,1,1,1]`, `"type":"mixed"`.
- **Isometric:** the docs say the terrain tool works on isometric maps. For the overlay to look right set the tileset's grid orientation to isometric and its grid width/height (section 1, `grid`).
- **Scripting check:** I painted through `TileLayer.wangEdit(wangSet).setCorner/setEdge` + `apply()` on orthogonal, isometric, staggered (x and y) and hex (x and y) maps, for corner, edge and mixed sets. All of them painted tiles, e.g. an isometric corner at vertex (2,2) filled cells (1,1)(2,1)(1,2)(2,2); the staggered-y and hex-y equivalents filled (2,0)(1,1)(2,1)(2,2). That proves the engine API accepts them, but it says nothing about the GUI brush's hex handling.
- **Hexagonal:** the terrain brush is **not really supported on hex maps.** GitHub issue #2824 (opened 2020, still open) records the maintainer's reason: the brush is built on 4 corners but hex tiles have 6. His suggested direction (2023) is to use 6 of the 8 `wangid` slots. A March 2026 comment proposes new serialization with no decision. A changelog entry mentions an improved terrain brush for *hexagonal maps with side length 0*, i.e. the staggered-like case (#3617). Nothing in the format has a hex-specific Wang type: a hex Wang set in 1.12.2 is the same 8-slot `wangid`. A game that wants hex terrain should treat the 8 slots as authoring data and not expect 6-sided semantics.
- Limits (docs): at most 254 colours, one tileset per set, a layer at a time. Flip/rotate tile transforms (the `transformations` element) widen what the brush can use.

## 6. Other surprises

1. **Hex rotation bits and order.** I rendered a marker tile with each flag. Screen angles clockwise from the marker's original corner: `0x20000000` rotates by 60, `0x10000000` by 120, both set = 180, `0x80000000` mirrors horizontally, `0x40000000` mirrors vertically. H + 120 gives 75 degrees from the flipped base, so flips apply first and the rotation applies after. Rotation is about the centre of the cell box; for a 32x32 image in a 32x32 cell the pivot is the image centre (images larger than the cell were not tested). Pointy and flat maps behave identically (the same image rotation, no axis dependence). Only these four rotations plus flips exist: 240 and 300 degrees are reached with flip + rotation combinations. The docs claim the order of operations does not matter on hex, but the measurements (H+120 at 75 degrees, not the -165 degrees you get from rotating first) disagree with that for the pairs I tried, so I would test your own order against Tiled.
2. `renderorder` is saved and loaded for every orientation but ignored by the non-orthogonal renderers, so an iso map that says `left-up` still draws top-to-bottom, left-to-right.
3. In an isometric map object coordinates scale by `tileheight` on both axes (not tilewidth/2): pixel = tile * `tileheight`.
4. **Isometric tile objects default to a bottom-centre anchor**; every other orientation uses bottom-left. Layer tiles on every orientation use the bottom-left of the cell box (iso: left vertex row, not centred).
5. Tall tiles overflow upward and are clipped by `toImage()`'s map bounds; Tiled's own export does not enlarge the image for overhang.
6. **Oblique tiles are not sheared**, only repositioned: the image bottom-left goes to the skewed (tx, ty+1) corner. Oblique tile objects likewise keep a bottom-left anchor at the skewed point.
7. Infinite maps: `width`/`height` 0; JSON layers carry `startx`/`starty`/`width`/`height` as bounds, while the TMX of a script-made layer wrote 0x0; unusual chunk sizes appear only via `editorsettings.chunksize`, and chunk origins are floor-aligned to it. Readers should take each chunk's own `width`/`height` rather than assume 16.
8. 1.12.2 writes `<grid>` even for orthogonal tilesets (`0x0` for script-made ones), and `<transformations>` only when set (`preferuntransformed` 0 or 1).
9. `skewx`/`skewy` of 0 are omitted; a reader must default them to 0. Negative skew is legal.
10. `hexsidelength` is written even when 0.
11. A hex tileset needs no special format: tile images are ordinary rectangles of the cell's bounding box (`tilewidth` x `tileheight`), including the transparent corners.
12. The Tiled scripting API crashed (segfault) on enumerating `MapObject` or constructor properties, and `tiled.loadWorld` returned undefined headless. Neither affects the saved files.

## 7. Measured 2026-11-02: odd sizes, degenerate hexagons, negative skews, the oblique order

What the sections above left open, measured with Tiled 1.12.2's scripting: `tests/fixtures/tilemap/tiled/scripts/make-fixtures.js` (its measure section) writes `tests/fixtures/tilemap/tiled/measured.json`, which `MapGrid`'s tests (`src/core/tilemap/grid_shape/tests.rs`) check every cell against. It holds `tileToScreen` of the cells -3..3 a side on 31 sample grids, where `toImage()` drew a solid tile image the cell's size at four cells of each bounded one, `screenToTile` at points round a hexagon, and the oblique draw order. The scripting names: `TileMap.skewX`/`skewY`, `hexSideLength`, `staggerAxis`/`staggerIndex`, `renderOrder`; a solid tile is `Tileset.addTile()` with `Tile.setImage(new Image(w, h, Image.Format_ARGB32))` filled; a render's pixels read through `Image.pixel(x, y)`.

1. **Staggered and hexagonal grids step in whole pixels.** Half a cell across rounds down, and a hexagon's slant, `(tile - side) / 2` along the stagger axis, rounds toward zero. So a 33 x 17 staggered grid steps 32 across and 8 down and shifts its rows by 16 (axis Y), or steps 16 across and 16 down and shifts its columns by 8 (axis X); 33 x 17 hexagons of side 13 step 32 across and 15 down (axis Y) or 23 across and 16 down (axis X), and of side 12 have a slant of 2 (axis Y) or 10 (axis X). The cell Tiled steps by is the box `2 * half` across and `2 * slant + side` along the axis: 32 x 16 for a 33 x 17 staggered cell. **A tile stands on that box's bottom**, so a 17-pixel tile reaches a pixel above its 16-pixel box (staggered y even 33 x 17, cell (1, 1): box top 8, image rows 7 to 23). Section 2's formulas hold with these steps for `tw/2`, `th/2` and `(th + side)/2`; the column step of an axis-Y grid is `2 * half` (32), not `tw` (33).
2. **Isometric grids step in half pixels from a whole-pixel origin.** `tileToScreen` steps 16.5 and 8.5 on 33 x 17 cells, and a bounded map's origin, `H * tw / 2`, rounds down: 49 for 3 rows of 33-pixel cells, not 49.5 (an infinite map's is 0). `toImage()` rounds where it draws an image: x half up, and on odd tile heights y one pixel lower where it is a whole pixel (floor plus one), so on 33 x 17 cells cell (1, 1), whose image's bottom-left is at (32.5, 34), draws it at (33, 35), and cell (2, 1), at (49, 42.5), at (49, 43). Odd widths over even heights (33 x 16) draw at y exactly. The engine places tiles at the exact point and leaves rounding to its pixel-art snap.
3. **A hexagonal map of side 0 is the staggered map of its stagger**: the same `tileToScreen` and the same renders (32 x 32 cells, both axes and indices).
4. **A side longer than the tile is saved and drawn.** The slant goes negative (-4 for sides 40 and 41 on 32-pixel cells, rounded toward zero), the step between rows is slant plus side (36, 37) and a tile stands on a box of `2 * slant + side` (32, 33), so the drawn rows spread apart with gaps between them. The scripting also takes a negative side (-5 on 32-pixel cells: slant 18, a row step of 13); the engine's `GridShape::Hexagonal::side` is unsigned, so the loader decides what a negative one does.
5. **Negative skews** give negative coordinates (skew x -10: cell (0, 1)'s corner at x -10), and `toImage()` draws from the content's leftmost and top-most point: its offset is (max(0, -H * skewx), max(0, -W * skewy)), (50, 0) for skew x -10 on a 5 x 5 map and (0, 60) for skew y -12.
6. **Oblique maps draw as orthogonal ones**: they honour all four render orders, rows outer and columns inner, whatever the skew ((8, 12), (-10, 0) and (0, -12)), unlike isometric, staggered and hexagonal maps. With right-down, the lower of two overlapping tall tiles ((3, 4) over (3, 3)) and the right of two wide ones ((4, 3) over (3, 3)) draw on top.
7. **`screenToTile` picks a hexagon by the nearest centre**, which on hexagons whose side is not the regular one hands a sliver by each vertex to a neighbour: on 32 x 32 hexagons of side 12 (axis y, odd), (31, 22.5) lies inside cell (0, 0)'s hexagon and Tiled picks (0, 1). On the shared edges it picked (0, 0) at the slanted edge's middle (24, 27), (1, 0) on the vertical side at (32, 16) and (0, 1) at the vertex (32, 22). The engine picks the outline the point is in.
