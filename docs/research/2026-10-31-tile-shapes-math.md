# Grid maths reference: hex, isometric, staggered

Legend: **[S]** = stated by the cited source (as fetched, via summaries; verify against the page before relying on exact wording). **[D]** = derived by me, not in a source; test before use.

---

## 1. Hex coordinates

Sources: https://www.redblobgames.com/grids/hexagons/ , https://www.redblobgames.com/grids/hexagons/implementation.html , https://www.redblobgames.com/grids/hexagons/derive-hex-neighbor-formula.html , https://www.redblobgames.com/grids/hexagons-v2/

### Cube and axial [S]
```
axial (q, r)  <->  cube (q, r, s)   with s = -q - r
cube -> axial: drop s
axial -> cube: s = -q - r
```

### Odd-r (pointy-top, odd rows shoved right by +1/2) [S, hexagons-v2]
```
oddr_to_cube(col, row):
    x = col - (row - (row & 1)) / 2
    z = row
    y = -x - z
cube_to_oddr(cube):
    col = x + (z - (z & 1)) / 2
    row = z
```

### Even-q (flat-top, even columns shoved down) [S, derive-neighbour page]
```
evenq_to_axial(col, row):
    q = col
    r = row - (col + (col & 1)) / 2
evenq_to_offset(q, r):
    col = q
    row = r + (q + (q & 1)) / 2
```

### Odd-q, even-r [D, by symmetry with the even-q and odd-r forms above; not fetched from a source]
```
oddq_to_axial(col, row):
    q = col
    r = row - (col - (col & 1)) / 2
evenr_to_axial(col, row):
    q = col - (row + (row & 1)) / 2
    r = row
```
Check odd-q against the even-q form. Red Blob's offset section also lists odd-r and even-r in a table; I could not retrieve that table.

Note: the `& 1` form is for two's-complement integers. For negative values, use floored division or Euclidean mod.

### Mapping to Tiled stagger
Tiled's hexagonal renderer treats the stagger axis like this [S, hexagonalrenderer.cpp]: stagger on Y shifts odd/even rows in x, and stagger on X shifts columns in y.

| Tiled `staggeraxis` | Shifted lines | Red Blob form | Pointy or flat |
|---|---|---|---|
| `y` | rows | odd-r / even-r | pointy-top |
| `x` | columns | odd-q / even-q | flat-top |

`staggerindex`: `odd` shifts odd-indexed lines along the stagger axis, `even` shifts even-indexed lines. So `y` + `odd` = odd-r, and `x` + `even` = even-q.
[D] This follows from the renderer's `doStaggerX/Y` checks, which the fetched excerpt did not define. Confirm with a test map before relying on it.

Tiled's `staggered` orientation (isometric with staggered axes) uses the same `staggeraxis`/`staggerindex` attributes [S, tmx-map-format].

---

## 2. Hex <-> pixel, rounding

Source: https://www.redblobgames.com/grids/hexagons/implementation.html , https://www.redblobgames.com/grids/hexagons-v2/ , https://www.redblobgames.com/grids/hexagons/

`size` is the centre-to-corner radius. `origin` is the pixel offset of the hex (0,0).

### Pointy-top (axial) [S]
```
hex_to_pixel:
    x = size * (sqrt(3) * q + sqrt(3)/2 * r) + origin.x
    y = size * (3/2 * r)                     + origin.y
pixel_to_hex:
    px = (x - origin.x) / size
    py = (y - origin.y) / size
    q = (sqrt(3)/3 * px - 1/3 * py)
    r = (2/3 * py)
    then hex_round(q, r)
```

### Flat-top (axial) [S, from implementation.html matrices]
```
hex_to_pixel:
    x = size * (3/2 * q)                     + origin.x
    y = size * (sqrt(3)/2 * q + sqrt(3) * r) + origin.y
pixel_to_hex:
    px = (x - origin.x) / size
    py = (y - origin.y) / size
    q = (2/3 * px)
    r = (-1/3 * px + sqrt(3)/3 * py)
    then hex_round(q, r)
```

Pointy and flat differ only in how the matrices are laid out. The start angle is 30° for pointy, 0° for flat.

### Doubled coordinates [S, hexagons page]
```
doublewidth (pointy):   x = size * sqrt(3)/2 * col ; y = size * 3/2 * row
doubleheight (flat):    x = size * 3/2 * col       ; y = size * sqrt(3)/2 * row
```
Pixel to doubled is the inverse [D]. The page says the same divisor is used, but gives no inverse code.

### Non-square-hex or arbitrary tile width/height (Tiled `hexsidelength`)
Tiled gives a hex with its own side length and arbitrary tile width and height [S, hexagonalrenderer.cpp]:
```
sideOffsetX = (tileWidth  - sideLengthX) / 2
sideOffsetY = (tileHeight - sideLengthY) / 2
columnWidth = sideOffsetX + sideLengthX
rowHeight   = sideOffsetY + sideLengthY
// recomputed hex-geometry sizes:
hexTileWidth  = columnWidth + sideOffsetX
hexTileHeight = rowHeight   + sideOffsetY
```
`sideLengthX` is used when staggering on X, otherwise `sideLengthY`. `hexsidelength` in the TMX sets this [S, tmx-map-format].

Tile to pixel [S, hexagonalrenderer.cpp, from the fetched summary]:
```
stagger X:
    pixelX = tileX * columnWidth
    pixelY = tileY * (tileHeight + sideLengthY) + (rowHeight if shifted(tileX))
stagger Y:
    pixelX = tileX * (tileWidth + sideLengthX) + (columnWidth if shifted(tileY))
    pixelY = tileY * rowHeight
```
`shifted()` is the odd/even test from `staggerindex`. Its definition was not in the fetched excerpt.

Pixel to tile [S, hexagonalrenderer.cpp]: subtract the stagger offset on the shifted lines, then take the reference cell by floor division. Compare the in-cell position with the candidate centres and pick the nearest by squared distance. The candidate offsets are the four cells around that reference. For stagger X they are (0,0), (+1,-1), (+1,0), (+2,0). For stagger Y they are (0,0), (-1,+1), (0,+1), (0,+2).

### Rounding to the nearest hex [S, implementation.html]
```
cube_round(qf, rf, sf):
    q = round(qf); r = round(rf); s = round(sf)
    dq = |q - qf|; dr = |r - rf|; ds = |s - sf|
    if dq > dr and dq > ds:  q = -r - s
    elif dr > ds:            r = -q - s
    else:                    s = -q - r
    return (q, r, s)
```
Cube rounding and the nearest-centre method pick the same cell for regular hexes. For non-regular hexes (Tiled's `hexsidelength`), the nearest-centre method is the one that matches the drawn tiles.

---

## 3. Neighbours, distance, line, range, rings

Sources: https://www.redblobgames.com/grids/hexagons/implementation.html , https://www.redblobgames.com/grids/hexagons/

### Cube direction vectors [S, implementation.html]
```
dirs = [(+1, 0,-1), (+1,-1, 0), ( 0,-1,+1), (-1, 0,+1), (-1,+1, 0), ( 0,+1,-1)]
// index 0..5, in the page's order; neighbour(h, d) = h + dirs[d mod 6]
// for axial, drop the s component
```
Confirm the index order against the page before relying on it for angular order (section 6 depends on it).

### Distance [S]
```
length(h)        = (|q| + |r| + |s|) / 2
distance(a, b)   = length(a - b)
```

### Line drawing [S, implementation.html]
```
N = distance(a, b)
a' = (a.q + 1e-6, a.r + 1e-6, a.s - 2e-6)   // nudge off the edges
b' = (b.q + 1e-6, b.r + 1e-6, b.s - 2e-6)
for i in 0..N:
    t = i / max(N, 1)
    out.push(cube_round(lerp(a', b', t)))
```

### Rings and range [S, hexagons page]
```
cube_ring(centre, R):
    hex = centre + dirs[4] * R
    for i in 0..6:
        for j in 0..R:
            out.push(hex); hex = neighbour(hex, i)
cube_spiral(centre, R) = [centre] ++ cube_ring(centre, 1..R)
hexes_in_range(N) = 1 + 3*N*(N+1)

range loop (axial):
    for q in -N..N:
        for r in max(-N, -q-N)..min(N, -q+N):
            out = centre + (q, r, -q-r)
```

---

## 4. Isometric (diamond), arbitrary tile width and height

Sources: https://clintbellanger.net/articles/isometric_math/ , https://github.com/mapeditor/tiled/blob/master/src/libtiled/isometricrenderer.cpp , https://doc.mapeditor.org/en/stable/reference/tmx-map-format/

### Tile to screen [S, isometricrenderer.cpp]
```
originX  = map.height * tileWidth / 2     // keeps the map's bottom-left at x >= 0
screenX  = (x - y) * tileWidth  / 2 + originX
screenY  = (x + y) * tileHeight / 2
```

### Screen to tile [S, isometricrenderer.cpp]
```
sx = screenX - originX
sy = screenY
tileX = sx / tileWidth + sy / tileHeight      // = x  (as derived below)
tileY = sy / tileHeight - sx / tileWidth      // = y
```
Derivation (checked): with `a = sx/tileWidth = (x-y)/2` and `b = sy/tileHeight = (x+y)/2`, we get `x = a + b` and `y = b - a`. This holds for any width:height ratio, not just 2:1.

Clint Bellanger's 2:1 form (TILE_WIDTH_HALF = 64, TILE_HEIGHT_HALF = 32) [S, isometric_math]:
```
screen.x = (map.x - map.y) * TILE_WIDTH_HALF
screen.y = (map.x + map.y) * TILE_HEIGHT_HALF
map.x = screen.x / TILE_WIDTH + screen.y / TILE_HEIGHT
map.y = screen.y / TILE_HEIGHT - screen.x / TILE_WIDTH
```
Bellanger's `TILE_WIDTH` in the float inverse is the full width. Check: tile (2,1) maps to (64, 96) at 128x64, and back.

Drawing a tile's image from its top-left: subtract tileWidth/2 from screenX [S, isometric_math]. Tiled anchors iso tile objects at bottom-centre [S, tmx-map-format].

### Draw order [S for Tiled's row-wise drawing, D for the depth rule]
Tiled draws tiles row by row, top to bottom on screen, and within a row left to right [S, isometricrenderer.cpp]. That gives correct overlap for a tile grid where the top-left screen row is further away.
[D] For sprites with height and objects, sort by `depth = x + y` (ascending), then by layer. Use `y`-sort on the screen y of the tile's bottom centre when objects and sprites are taller than a tile.

---

## 5. Staggered isometric

Sources: https://github.com/mapeditor/tiled/blob/master/src/libtiled/staggeredrenderer.cpp (partial fetch), https://doc.mapeditor.org/en/stable/reference/tmx-map-format/

[S] Tiled's staggered renderer shares the hex-style parameters (`staggerX`, `staggerEven`, `sideOffsetX/Y`, `columnWidth`, `rowHeight`). Its screen-to-tile code starts with a reference cell from floor division, doubles the stagger-axis index, and then picks a neighbour by comparing `rel.y` with `sideOffsetY` and a diagonal helper `rel.x * (tileHeight / tileWidth)`. The result is refined with a rotation of -45° and a divide by `tileWidth / sqrt(2)`.

[D] A simpler derivation, which I have not verified against Tiled's output, is the rectangular brick layout:
```
// staggeraxis = y (rows shifted by half a tile width in x)
screenX = tileX * tileWidth  + (shifted(tileY) ? tileWidth/2 : 0)
screenY = tileY * tileHeight/2
```
With iso diamond tiles, that layout needs the 2:1 ratio. For an arbitrary ratio, follow Tiled's renderer.

Neighbours [D]: for a staggered grid, the neighbours of a cell depend on the parity of the shifted line. The shifted-line rule is the same as in section 1 (the odd-r pattern is the one that applies).

Draw order [D]: use the same depth rule as section 4, with `depth = screenY` for staggered grids because the stagger keeps each row's tiles in one screen row.

---

## 6. Boundaries: edges and corners of hex regions

Source: https://www.redblobgames.com/grids/parts/ , plus derivation.

### What the parts article covers [S]
- Each hex has 6 edges and 6 vertices. Each hex labels only 2 vertices (north and south). Each vertex is shared by 3 hexes.
- Edges are shared between neighbours. Each edge is referenced from both sides.
- The article gives the relations **Borders** (face to edges), **Joins** (edge to faces), **Corners** (face to vertices) and **Endpoints** (edge to vertices). For squares, **Touches** gives the faces around a vertex, including diagonals.
- The article does **not** give a region-boundary algorithm, outline chaining, hole handling or diagonal-contact rules.

### Derived method [D]
```
// Edge identity: an edge is (cell, d). It is the edge facing direction d.
// A boundary edge is (c, d) where c is filled and neighbour(c, d) is empty or out of bounds.

boundary_edges = [ (c, d) for c in filled for d in 0..5
                   if not filled(neighbour(c, d)) ]

// Vertex identity: corner i of hex H is shared by H, neighbour(H, i) and neighbour(H, i+1).
// Key the vertex by the sorted set of those 3 cells (or by the canonical vertex label).

// Each boundary edge has two endpoints (its two corners). Orient each boundary edge
// so the filled cell is on its left. Then at each vertex, outgoing edges are matched to
// incoming ones and the loop is walked until it returns to its start.
```

Orientation and the 3-cell vertex key depend on the direction ordering. Confirm the angular order against the page before fixing it in code.

### Holes and touching corners [D]
- **Holes:** a boundary loop with negative signed area (under the same orientation rule) is a hole inside its enclosing outer loop. Pair each hole with the outer loop that contains it.
- **Touching corners (pinch):** at a vertex where two filled hexes meet diagonally and the third is empty, two boundary edges leave the same vertex. Pick one rule and apply it everywhere: either always turn toward the filled side (joins diagonal cells into one loop) or always split (two loops). Collision outlines usually want the split form, so that the two cells collide separately.

---

## 7. Rotation and reflection

Source: https://www.redblobgames.com/grids/hexagons/implementation.html (rotation), https://www.redblobgames.com/grids/hexagons/ (cube section)

### Rotation in cube coordinates [S]
```
rotate_right(q, r, s) = (-r, -s, -q)      // 60 degrees, one way
rotate_left(q, r, s)  = (-s, -q, -r)      // 60 degrees, the other way
rotate_120 = rotate applied twice
```
"Which is left and which is right depends on your map orientation" [S]. Swap them if the orientation differs.

In axial coordinates (drop s), 60 degree steps are derived by converting to cube and back [D].

### Reflection in cube coordinates [S, hexagons page]
```
reflect_q(h) = (h.q, h.s, h.r)
reflect_r(h) = (h.s, h.r, h.q)
reflect_s(h) = (h.r, h.q, h.s)
```

### Tiled GID flags [S, global-tile-ids]
```
FLIPPED_HORIZONTALLY   = 0x80000000
FLIPPED_VERTICALLY     = 0x40000000
FLIPPED_DIAGONALLY     = 0x20000000
ROTATED_HEXAGONAL_120  = 0x10000000
clear all four:  gid & ~0xF0000000
```
Tiled's text says the 0x10000000 flag "indicates 120 degrees clockwise rotation", and that the 0x20000000 flag, on hexagonal maps, means "rotated 60 degrees clockwise". The flag is ignored on non-hex maps. The page's bit numbers are inconsistent with its hex values, so use the hex values.

### Rotating hex tile art [not found]
Neither Red Blob page covers rotating sprites or tile art. How a 120-degree GID flag is applied to the art (pre-rendered rotated sprites, or a transform at draw time) was not in any source I could fetch.

---

## Gaps and sources not retrieved

- Red Blob's full offset-table section (neighbour tables per parity, odd-q/even-r formulas as printed) did not come through in any fetch. The odd-q and even-r forms above are derived.
- Tiled's `staggeredrenderer.cpp` tile-to-screen code and the `doStaggerX/Y` definitions were not in the fetched excerpts.
- No source for rotating hex tile art.
- Red Blob's parts article has no boundary-tracing or hole algorithm. Section 6 is derived.
- Fetched content came through summarised. Quoted fragments in this file are paraphrases of the source where marked [S]. Check exact wording and constants on the page before copying them into code.

## Sources

- https://www.redblobgames.com/grids/hexagons/
- https://www.redblobgames.com/grids/hexagons/implementation.html
- https://www.redblobgames.com/grids/hexagons/derive-hex-neighbor-formula.html
- https://www.redblobgames.com/grids/hexagons-v2/
- https://www.redblobgames.com/grids/parts/
- https://doc.mapeditor.org/en/stable/reference/tmx-map-format/
- https://doc.mapeditor.org/en/stable/reference/global-tile-ids/
- https://clintbellanger.net/articles/isometric_math/
- https://github.com/mapeditor/tiled/blob/master/src/libtiled/hexagonalrenderer.cpp
- https://github.com/mapeditor/tiled/blob/master/src/libtiled/staggeredrenderer.cpp
- https://github.com/mapeditor/tiled/blob/master/src/libtiled/isometricrenderer.cpp
