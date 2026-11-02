# Tile Maps: 6b and Later

Sub-project 6a (`docs/specs/2026-10-29-tilemap-design.md`) made orthogonal tile maps from Tiled and LDtk: registry assets with hot reload, chunked drawing, collision and shadows from the tiles, objects as entities, runtime edits with Wang terrain, worlds. Every map 6a cannot draw correctly fails to load with an error naming the feature and sub-project 6b, rather than loading wrong. The owner chose to do isometric, hexagonal and infinite maps straight after 6a; this is 6b's scope, what each feature needs from 6a's model, and what stays later. What 6a measured, and its limits, are in `docs/backlog/tilemap.md`.

## 6b: what it adds

| Feature | Tiled | LDtk |
|---|---|---|
| Isometric maps | `orientation: isometric` (diamond cells, 2:1 or any tile ratio) | none (LDtk is orthogonal only) |
| Staggered isometric maps | `orientation: staggered`, with `staggeraxis` (x or y) and `staggerindex` (odd or even) | none |
| Hexagonal maps | `orientation: hexagonal`, with `staggeraxis`, `staggerindex` and `hexsidelength` | none |
| Oblique maps | Tiled 1.12's new orientation (a skewed grid; read its parameters from Tiled 1.12's format when 6b starts) | none |
| Infinite maps | `infinite: true`: tile layers saved as chunks (`chunks: [{ x, y, width, height, data }]`, coordinates that may be negative), and the map's `width` and `height` meaningless | none |

Infinite maps load (`an_infinite_map_s_chunks_land_where_tiled_put_them` replaced the two tests that pinned their refusal). The orientations are still refused in `tilemap/tiled/build.rs` (both formats), pinned by `isometric_hexagonal_staggered_and_oblique_maps_fail_naming_6b`. 6b replaces each refusal with the feature, and keeps refusing what it does not do. The anti-goal stands: no orientation value the engine accepts but does not implement. Maps built in code are where the grids arrive first: `TileMap::with_shape` takes any of them, every tile layer carries its `MapGrid` (`grid_shape.rs`), and class tiles, picking (`world_to_cell`, `cell_to_world`) and a stream's `ChunkRequest` go through it; drawing, collision and shadows still place such a map's cells as rectangles of its cell size until they move onto the grid.

## What each needs from 6a's model

### Orientation, everywhere a cell meets a pixel

6a assumes a cell `(x, y)` covers the rectangle `(x, y) * cell_size`. That assumption lives in a few places, which an orientation on the map (a `TileMap` field, copied to the layers as they spawn) generalises:

- **Cell and world conversions:** `edit.rs` (`world_to_cell`, `cell_to_world`), and through them `MapsView`. Isometric is an affine map of the orthogonal grid; staggered and hexagonal maps need offset-coordinate conversions, and a point to cell lookup that picks the nearest hexagon.
- **Drawing** (`render/tiles.rs`, `chunk_instances`): an instance's place in layer pixels, the tile's anchor in its cell (Tiled draws an isometric tile from its cell's bottom centre), the draw order inside a layer (isometric rows by `x + y`, staggered and hex rows in their own order), a chunk's box for culling (a diamond's or a staggered band's bounding box), and y-sorting by those rows rather than by grid rows. The slot uniform (map transform, parallax, the tileset's place) needs nothing new.
- **Objects** (`tiled/objects.rs`): Tiled writes an isometric map's object positions in its projected space (both axes in tile-height units along the diamond's axes), so positions and polygons convert on load; `MapObject` and its `Transform` stay as they are.
- **Flips:** gid bit 28 is Tiled's 120° rotation of hexagonal tiles, which 6a's `decode_gid` clears. A cell already keeps four flip bits (28 to 31), so a hex rotation flag fits the 4-byte cell; `TileFlip` gains it.
- **Wang terrain** (`wang.rs`): corner and edge colours are in cell space, so isometric and staggered maps keep the rules; check against Tiled what its terrain brush does on hexagonal maps before porting it, as 6a did for orthogonal ones.

### Collision and shadows

- **Isometric:** the diamond grid is an affine image of the square one, so `TileColliders` could keep tracing in cell space with its colliders under the map's linear map: a 45° turn under a non-uniform scale is a 2:1 diamond, which a child entity under a scaled parent expresses. Check first that physics builds every collider shape correctly under such a sheared world map (`physics/shape.rs` builds Rapier shapes under a collider's linear map; the character sweep and one-way normals must hold too). The slopes and shapes of a tile, drawn in Tiled over the tile's image, convert to cell space on load. Occluders follow the same map (`Boundary` and the chains are in cell space too).
- **Staggered and hexagonal:** a hexagonal cell has six neighbours, so `TileColliders`' square outline trace (units one or two cells decide, `physics/tiles/outline.rs`) does not apply. 6b needs a hex grid kind that traces the boundary of hexagons the same way (shared edges cancel, loops stay seamless, an edit retraces only the loops through it, packing by chunk), or per-cell polygons merged across shared edges. This is the largest piece of 6b, and it is physics', with the character controller's floor and wall rules checked on hex outlines.
- **Navigation** (`NavigationGrid::block_tiles`) reads a square grid; isometric maps can block by cell, hex maps need a hex graph or a sampled square grid.

### Infinite maps

- **A sparse grid (done):** `TileGrid` is a map of 32-cell chunks by signed chunk coordinate with per-chunk versions, bounded (a fixed size) or unbounded, so the chunk renderer, collision and occluders read versions as they did. `world_to_cell` gives cells outside a layer, and an edit past the current chunks of an unbounded layer creates them.
- **Collision (done):** `TileColliders` holds the chunks its layers have, signed, with no cap on a side; a cell collides within 186,411 cells of (0, 0) on either axis (outline points stay `i32`). Packing is per chunk, so colliders stay one per chunk. A reach past that is not built (see `docs/backlog/tilemap.md`, Limits).
- **Loading (done):** the reader decodes each chunk (every encoding 6a reads) into an unbounded grid; `TileMap::infinite` marks the map, its `size` is zero, and `TileMap::bounds()` is the rectangle of the cells its tile layers hold tiles in (`MapsView::bounds` for a spawned map as edited).
- **Worlds (done):** a Tiled world places an infinite map by its tile origin, its rectangle its tiles' bounds whatever size the world lists (`WorldLevel::origin`).
- **Streaming (done):** `MapStream` on a map root generates chunks on its own thread around the cameras and `StreamAnchor`s, lands and unloads them within per-frame budgets, keeps the player's changes in `SavedChunks`, and streams a Tiled infinite map's world around its hand-made chunks (`ChunkEvent`, `ChunkOwned`, `MapsView::stream_ready` and `stream_status`; see the ryo-tilemap skill and `docs/backlog/tilemap.md`).

## Later than 6b

- Running Tiled's automapping and LDtk's auto-layer rules at runtime: both run in their editors, which save the tiles.
- Per-tile runtime overrides of tileset data (Godot's runtime tile data).
- Saving maps back to the editors' formats, and a tile editor inside the engine.
- Tiled's text objects beyond their placement (`ObjectShape::Text` keeps the text; nothing draws it).
- Streaming a world's levels automatically: 6a gives positions, neighbours and `spawn_levels_in`, and when to load and unload stays the game's call.
- LDtk level background images.
- What `docs/backlog/tilemap.md` lists as limits: the landing frame's first draw, relabelling long split loops, lighting's lack of a spatial index, and the keep window's memory.

## Outside tile maps

The 2026-02 survey of Godot, Unity, GameMaker, Defold, Bevy, Love2D, Phaser and Pygame also noted gaps outside tile maps. Skeletal animation, joints and capsule, polygon, segment and compound colliders have been built since; these were still open then (check before working on one): localization (string tables, plural rules, locale switching), accessibility (screen readers, text to speech), sprite atlas packing at build time, normal-map lighting for 2D sprites, tweening along Bézier and spline paths, and texture compression at build time (DXT, ETC2, ASTC).
