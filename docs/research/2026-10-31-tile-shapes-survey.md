# Isometric, hexagonal and infinite tile maps: engine survey

Research date: 2026-10-31. Versions are the ones each official page states. Where a claim could not be verified from an official page, it is marked **[unverified]** and the gap is listed at the end of each section.

Summary of the survey's main points:

- Godot 4.7 ships four tile shapes (square, isometric, half-offset square, hexagon) with layouts and an offset axis, and its TileSet carries physics, navigation and occlusion layers per tile. Its docs say nothing about infinite maps, and its pathfinding grid covers square and isometric only.
- Unity 6.6 has a Grid component with Rectangle, Hexagon, Isometric and Isometric Z as Y layouts. Its Tilemap Renderer's Chunk versus Individual mode decides whether characters can sit between tiles, and it ships Rule Tile variants for hex and iso.
- bevy_ecs_tilemap 0.20 covers square, isometric and hex. bevy_ecs_tiled loads iso, hex and infinite Tiled maps, but its physics on diamond-iso maps has had open position bugs.
- Most other engines are weak here. Defold's built-in tilemap is rectangular only, Excalibur renders iso but not hex, and MonoGame.Extended supports iso only, with no infinite maps.
- Depth sorting with characters on iso is the most reported pain point across engines. Every engine that works well on iso has an explicit answer for it: per-tile Y-sort origin (Godot), Individual mode or Z-as-Y with a transparency axis (Unity), per-entity depth from Y (Phaser, Excalibur).

---

## 1. Godot 4.7 (TileMapLayer, TileSet)

Sources read: [Using TileMaps](https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html) (page header says 4.7), [TileSet class](https://docs.godotengine.org/en/stable/classes/class_tileset.html) (4.7), [TileMapLayer class](https://docs.godotengine.org/en/stable/classes/class_tilemaplayer.html) (4.7), [AStarGrid2D class](https://docs.godotengine.org/en/stable/classes/class_astargrid2d.html) (4.7).

**Shapes and variants**
- `TileSet.TileShape`: `TILE_SHAPE_SQUARE`, `TILE_SHAPE_ISOMETRIC`, `TILE_SHAPE_HALF_OFFSET_SQUARE`, `TILE_SHAPE_HEXAGON` ([TileSet class](https://docs.godotengine.org/en/stable/classes/class_tileset.html)).
- `TileSet.TileLayout`: `STACKED`, `STACKED_OFFSET`, `STAIRS_RIGHT`, `STAIRS_DOWN`, `DIAMOND_RIGHT`, `DIAMOND_DOWN`.
- `TileSet.TileOffsetAxis`: `HORIZONTAL`, `VERTICAL`. This applies to all half-offset shapes (hex, iso, half-offset square) ([TileSet class](https://docs.godotengine.org/en/stable/classes/class_tileset.html)).
- `tile_size` is the bounding rectangle of the shape and sets the minimum atlas cell size ([TileSet class](https://docs.godotengine.org/en/stable/classes/class_tileset.html)).
- How a game picks one: the shape, layout and offset axis are set on the TileSet, so one project can have several tilesets with different grids. Community reports on the Godot forum show a working hex setup as Shape Hexagon, Layout Stacked, Offset Axis Horizontal ([Godot forum, hex coordinate thread](https://forum.godotengine.org/t/hex-tilemap-coords-from-mouse-click-event-are-offset/119767)).

**Coordinate APIs (TileMapLayer, Godot 4.7)**
- `local_to_map(local_position)` and `map_to_local(map_position)`. For global positions, call `to_local()` / `to_global()` first. `map_to_local` returns the cell centre and ignores `TileData.texture_origin` ([TileMapLayer class](https://docs.godotengine.org/en/stable/classes/class_tilemaplayer.html)).
- `get_neighbor_cell(coords, neighbor)` takes the grid layout into account. `get_surrounding_cells(coords)` returns 4 cells on square and 6 on hex ([TileMapLayer class](https://docs.godotengine.org/en/stable/classes/class_tilemaplayer.html)).
- Also `get_used_rect()`, `get_used_cells()`, `get_used_cells_by_id()`, and pattern get/set/map ([TileMapLayer class](https://docs.godotengine.org/en/stable/classes/class_tilemaplayer.html)).
- No line or area helper (no `get_cells_line`, no range or distance API) is documented on the TileMapLayer page ([TileMapLayer class](https://docs.godotengine.org/en/stable/classes/class_tilemaplayer.html)). Distance on hex or iso is left to user code.

**Draw order and depth sorting**
- Each TileSet tile has a per-tile **Y Sort Origin** (pixel offset). Y-sort must be enabled on the layer (`Y Sort Enabled` under CanvasItem), with `X Draw Order Reversed` as an option ([Using TileMaps](https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html)).
- The TileSet docs recommend Y-sort for isometric sets, with all sibling TileMapLayers and their parent Node2D set to Y-sort ([TileSet class](https://docs.godotengine.org/en/stable/classes/class_tileset.html), via search snippet of 4.x docs).
- Y-sort only works between siblings in the same z_index. Forum answers say a character must be a child of the tilemap, or share its parent with matched z-indices, for Y-sort to work ([Godot forum](https://forum.godotengine.org/t/why-do-you-have-to-parent-a-node-to-a-tilemap-in-order-for-its-ysort-to-work-in-an-isometric-game/25776), [Godot forum, 4.3](https://forum.godotengine.org/t/unable-to-properly-y-sort-entities-in-a-multi-leveled-isometric-tilemap/93061)).
- `rendering_quadrant_size` (default 16) groups tiles into canvas items. Y-sorted layers group by Y position instead ([TileMapLayer class](https://docs.godotengine.org/en/stable/classes/class_tilemaplayer.html)).

**Collision, navigation, occlusion (per tile)**
- TileSet has per-layer lists of physics layers (collision layer, mask, priority, material), navigation layers, and occlusion layers (light mask, SDF collision). Per-tile shapes are set on each tile's data ([TileSet class](https://docs.godotengine.org/en/stable/classes/class_tileset.html)).
- `physics_quadrant_size` (default 16) merges collision shapes within each quadrant. Larger values reduce precision of `get_coords_for_body_rid()`. Set it to 1 for exact cell coordinates ([TileMapLayer class](https://docs.godotengine.org/en/stable/classes/class_tilemaplayer.html)).
- Navigation: the layer-level navigation docs say built-in TileMap navigation has performance and path-following limits, and suggest baking to NavigationRegion2D ([Using TileMaps](https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html)).
- Slopes: no documentation found [unverified].
- Hex and iso pathfinding: `AStarGrid2D` supports `CELL_SHAPE_SQUARE`, `CELL_SHAPE_ISOMETRIC_RIGHT` and `CELL_SHAPE_ISOMETRIC_DOWN`. It does not list a hex shape, and its neighbour rules for iso are not described ([AStarGrid2D class](https://docs.godotengine.org/en/stable/classes/class_astargrid2d.html)). Hex pathfinding is left to user code.

**Autotiling (terrain)**
- Terrain sets exist on the TileSet with a mode. The TileMapLayer page documents Connect mode (neighbours on the same layer) and Path mode (within one stroke), plus per-tile overrides ([Using TileMaps](https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html)).
- The docs do not say whether terrain works on hex or iso [unverified]. Community guides on terrain are square-grid ([bugnet.io](https://bugnet.io/blog/fix-godot-tileset-terrain-not-painting)) and no hex terrain bug was found.

**Unbounded or large maps**
- TileMapLayer is described as "optimized for drawing large numbers of tiles" with quadrant batching. No infinite or chunk-streaming API is documented ([Using TileMaps](https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html)).
- No Tiled infinite-map import is in the official docs. Chunk loading in Godot is community work, for example a chunk loader asset that streams TileMapLayer data around the player ([Godot asset library](https://godotengine.org/asset-library/asset/edit/23202)).

**Run-time editing**: `set_cell` / `erase_cell` on TileMapLayer; the `changed` signal can fire often in batch edits and the docs say to defer heavy handlers. `update_internals()` forces an immediate update and is expensive ([TileMapLayer class](https://docs.godotengine.org/en/stable/classes/class_tilemaplayer.html)).

**Known pain points (Godot)**
- Y-sort of multi-tile walls flickers or reverts. Issue opened January 2023 on 4.0 beta; closed June 2024 via PR #92787 for 4.3. A maintainer said there was "not really a fix" without rendering-server changes ([godot#71257](https://github.com/godotengine/godot/issues/71257)). A later comment says toggling `X Draw Order Reversed` fixes it on the layer ([godot#71257](https://github.com/godotengine/godot/issues/71257)).
- Tile overlap follows quadrant batching; the maintainer's answer is to enable Y-sort on the layer ([godot#54006](https://github.com/godotengine/godot/issues/54006)).
- Hex click coordinates offset with the camera, and global/local mix-ups. The fix in forum threads is to call `to_local()` before `local_to_map()` ([Godot forum](https://forum.godotengine.org/t/hex-tilemap-coords-from-mouse-click-event-are-offset/119767), [Godot forum](https://forum.godotengine.org/t/aligning-tilemap-with-underlying-hex-grid/102169), [godot#37394](https://github.com/godotengine/godot/issues/37394)).
- Large tilesets and big maps lag in the editor and at runtime ([Godot forum](https://forum.godotengine.org/t/performance-issues-of-tilemaplayer-when-using-a-large-tileset/113746), [Godot forum](https://forum.godotengine.org/t/performance-issues-with-lots-of-tilemaplayers-for-large-3d-stacked-worlds/116503)).
- Navigation agents avoid whole tiles instead of tile collision shapes, a question in the forum ([Godot forum](https://forum.godotengine.org/t/avoiding-obstacles-with-tilemaplayer/93475)).

**Gaps**: hex terrain, slope handling and infinite maps are not in the official pages read. Hex-specific add-ons exist on the Godot asset store ([Godot store](https://store.godotengine.org/asset/zehir/hexagon-tilemaplayer/)); their quality was not checked.

---

## 2. Unity 6 (Grid, Tilemap, Tilemap Renderer, colliders, Rule Tiles, bounds)

Sources read: [Grid reference](https://docs.unity.com/en-us/engine/current/manual/unity2d/tilemaps/reference/grid) (6.6), [Hexagonal tilemap](https://docs.unity.com/en-us/engine/6000.6/manual/unity2d/tilemaps/hexagonal-tilemap) (6.6), [Isometric tilemap renderer modes](https://docs.unity.com/en-us/engine/6000.6/manual/unity2d/tilemaps/isometric-tilemap/tilemap-renderer-isometric-modes) (6.6), [Tilemap Renderer reference](https://docs.unity.com/en-us/engine/6000.0/manual/unity2d/tilemaps/reference/tilemap-renderer) (6.0), [CompressBounds](https://docs.unity.com/en-us/engine/current/script-reference/unityengine/tilemaps/tilemap/compressbounds) (6000.6), [2D Extras API](https://docs.unity3d.com/Packages/com.unity.2d.tilemap.extras@3.0/api/UnityEngine.html) (extras 3.0).

**Shapes and variants**
- Grid Cell Layout: Rectangle, Hexagon, Isometric, Isometric Z as Y ([Grid reference](https://docs.unity.com/en-us/engine/current/manual/unity2d/tilemaps/reference/grid)).
- Cell Swizzle: XYZ, XZY, YXZ, YZX, ZXY, ZYX, which remaps axes for flat or facing grids ([Grid reference](https://docs.unity.com/en-us/engine/current/manual/unity2d/tilemaps/reference/grid)).
- Hex has two orientations on the palette: Hexagonal Point Top and Hexagonal Flat Top. Flat Top swaps the cell size axes and offsets every other column ([Hexagonal tilemap](https://docs.unity.com/en-us/engine/6000.6/manual/unity2d/tilemaps/hexagonal-tilemap)).
- How a game picks one: the Grid's Cell Layout, with a matching tile palette type.

**Coordinate APIs**: `Grid.CellToWorld` and `Grid.WorldToCell` exist in scripting. The 6.6 Grid page does not give the layout formulas or describe the differences between layouts [unverified].

**Draw order and sorting** ([Isometric renderer modes](https://docs.unity.com/en-us/engine/6000.6/manual/unity2d/tilemaps/isometric-tilemap/tilemap-renderer-isometric-modes), [Tilemap Renderer reference](https://docs.unity.com/en-us/engine/6000.0/manual/unity2d/tilemaps/reference/tilemap-renderer)):
- Mode **Chunk** (default) batches tiles and keeps them sorted, but other objects can't sit between tiles at different depths. Tiles with several textures can sort wrong; the docs say pack them into one atlas. Chunk is not compatible with the SRP Batcher on the 6.0 page.
- Mode **Individual** sorts tiles and sprites together so characters can sit between tile layers, at a performance cost.
- Sort Order: Bottom Left (default), Bottom Right, Top Left, Top Right.
- Isometric: raise Order in Layer for each new tilemap so it draws in front of the one below.
- Z as Y: set the 2D renderer's Transparency Sort Mode to Custom Axis, with Transparency Sort Axis z = `Cell Size.y × -0.5 − 0.01` (for example -0.26 when Cell Size y is 0.5). Without this, higher tiles can draw in front of lower ones.
- Chunk Culling Bounds: Auto or Manual. Use Manual when Auto culls large sprites.

**Collision**
- The Tilemap Collider 2D builds each tile's shape from the grid cell, and the cell shape follows the Cell Layout ([Unity manual, 2018.3 archive](https://docs.unity3d.com/es/2018.3/Manual/class-TilemapCollider2D.html)). The 2018 page is the only source found, and no Unity 6 page on hex or iso collider shapes could be fetched [unverified for 6.x].
- Composite Collider 2D is paired with the Tilemap Collider 2D on the same tilemap. When Used by Composite is on, the Tilemap Collider's properties are controlled by the Composite ([Unity manual, 2018.3 archive](https://docs.unity3d.com/es/2018.3/Manual/class-TilemapCollider2D.html)).
- Slopes: no documentation found [unverified].

**Autotiling (Rule Tiles)**: the extras package ships `HexagonalRuleTile`, `IsometricRuleTile`, and templated `<T>` variants that take a custom neighbour rule class ([2D Extras API](https://docs.unity3d.com/Packages/com.unity.2d.tilemap.extras@3.0/api/UnityEngine.html)). Their neighbour matching is not documented there.

**Unbounded maps**
- A Tilemap's bounds grow when `SetTile` writes outside them. Erasing tiles does not shrink them. `CompressBounds()` shrinks `origin` and `size` to the cells that contain tiles ([CompressBounds](https://docs.unity.com/en-us/engine/current/script-reference/unityengine/tilemaps/tilemap/compressbounds)). Unity's bug tracker describes the non-shrinking behaviour as intentional ([Unity issue tracker](https://issuetracker.unity3d.com/issues/tilemap-dot-size-does-not-change-when-the-tiles-are-erased)).
- No infinite-map feature is documented. Chunk streaming is done by user code.

**Known pain points (Unity)**
- Isometric sorting of characters in Chunk mode is the top complaint. Forum answers say Individual mode fixes it; one user suspects a Unity update changed the default to Chunk ([Unity forum, 2025](https://discussions.unity.com/t/isometric-tilemap-sorting-issues/714645)). Tall characters in Z-as-Y still mis-sort, with no solution reported ([Unity forum](https://discussions.unity.com/t/issues-sorting-character-in-z-as-y-isometric-tilemap-issues-sorting-a-character-inside-a-isometric-z-as-y-tilemap/247491)).
- Iso sort bugs with Bottom Right and Top Left sort orders and custom cell sizes with multiple chunks ([Unity bug tracker](https://issuetracker.unity3d.com/issues/isometric-tilemap-incorrectly-sorts-tiles-when-sort-order-is-set-to-bottom-right-or-top-left)).
- Composite Collider problems: colliders not merging, rotated tiles splitting partitions, inverted tiles failing; a forum workaround is to toggle the Tilemap Collider or regenerate the Composite ([Unity issue tracker](https://issuetracker.unity3d.com/product/unity/issues/guid/974119), [gamedev.tv](https://community.gamedev.tv/t/collider-problems/195688)).
- Bulk writes: `SetTile` is slow when many tiles are written and colliders are attached; `SetTilesBlock` is faster. One report of 20 minutes for 786k tiles on 2018 Unity with custom tile classes ([Unity forum](https://forum.unity.com/threads/tilemap-settilesblock-performance.537274/), [Unity forum](https://forum.unity.com/threads/settile-is-too-slow.1278899/)).

**Gaps**: the Unity 6 pages for Tilemap Collider 2D and the Grid layout formulas could not be fetched. Hex collider and Rule Tile behaviour on hex or iso rests on the 2018 page and the 2D Extras API.

---

## 3. Bevy ecosystem

### bevy_ecs_tilemap (0.20.0, Bevy 0.20)

Sources: [repository README](https://github.com/StarArawn/bevy_ecs_tilemap), [docs.rs crate page](https://docs.rs/bevy_ecs_tilemap/latest/bevy_ecs_tilemap/) (0.20.0), [TilemapType](https://docs.rs/bevy_ecs_tilemap/latest/bevy_ecs_tilemap/map/enum.TilemapType.html).

- Map types: `TilemapType::Square`, `Hexagon(HexCoordSystem)`, `Isometric(IsoCoordSystem)`. The enum variants (for example row/column and diamond/staggered) were not readable in the fetched pages [unverified].
- Each tile is an entity, and rendering is chunked. Listed features include layers, sparse maps, GPU animation, texture arrays, and Tiled export support ([crate page](https://docs.rs/bevy_ecs_tilemap/latest/bevy_ecs_tilemap/)).
- Examples in the repo include `hexagon_column` (flat-top), `hexagon_row` (pointy-top), `iso_diamond`, and `iso_staggered` ([repository README](https://github.com/StarArawn/bevy_ecs_tilemap)).
- Chunking: the README describes chunked meshes sent to the GPU. A chunking example builds an infinite map from chunks ([repository README](https://github.com/StarArawn/bevy_ecs_tilemap)).
- Coordinate helpers, neighbour lookup and collision: not described on the crate page [unverified]. The crate exposes `TilemapRenderSettings` for chunk size; the bevy_ecs_tiled FAQ says Y-sort and chunk size for diamond iso may need tweaking ([bevy_ecs_tiled FAQ](https://adrien-bon.github.io/bevy_ecs_tiled/FAQ.html)).
- Pathfinding, autotile and run-time editing rules: not covered in the sources read [unverified].

### bevy_ecs_tiled

Sources: [repository README](https://github.com/adrien-bon/bevy_ecs_tiled), [FAQ](https://adrien-bon.github.io/bevy_ecs_tiled/FAQ.html), issues [#48](https://github.com/adrien-bon/bevy_ecs_tiled/issues/48).

- Orientations: orthogonal, isometric (diamond), hexagonal. Staggered isometric is not supported (issue #31, cited in the FAQ) ([repository README](https://github.com/adrien-bon/bevy_ecs_tiled)).
- Infinite maps: supported, finite or infinite layers, external or embedded tilesets ([repository README](https://github.com/adrien-bon/bevy_ecs_tiled)). Issue #46, a panic on infinite-map load, is listed on a third-party mirror as open, with a related fix PR closed [unverified against GitHub].
- Collision: Rapier or Avian colliders for tiles and objects, with a custom backend option ([repository README](https://github.com/adrien-bon/bevy_ecs_tiled)). Whether colliders come per tile or from object layers is not stated.
- Known iso and hex collider problems: [issue #48](https://github.com/adrien-bon/bevy_ecs_tiled/issues/48) (read directly) is closed as completed in August 2025. The maintainer found the cause was an isometric orientation on the tileset itself, which is unsupported, and opened issue #129 to track it. Diamond iso collider shape and position issues (#32, #40) appear on a third-party mirror [unverified against GitHub].
- Hex: the maintainer closed a hex-and-iso collider positioning bug (#40 per the mirror) [unverified].
- The README says the project was built with an LLM coding agent and a human reviews each commit ([repository README](https://github.com/adrien-bon/bevy_ecs_tiled)).

---

## 4. Phaser 3 (and 4)

Sources: [Tilemap class](https://photonstorm.github.io/phaser3-docs/Phaser.Tilemaps.Tilemap.html) (legacy 3.51 docs), [depth sorting example](https://phaser.io/oss/examples/v4.1.0/depth%20sorting.md) (v4.1.0), [devlog 110](https://phaser.io/devlogs/110), [issue #6215](https://github.com/phaserjs/phaser/issues/6215).

- Orientations: as of Phaser 3.50 the Tiled import supports orthogonal, isometric, hexagonal and staggered maps. Before 3.50 only orthogonal ([Tilemap class](https://photonstorm.github.io/phaser3-docs/Phaser.Tilemaps.Tilemap.html)).
- Coordinate APIs: `worldToTileX/Y(worldX, snapToFloor, camera, layer)`, `tileToWorldX/Y(tileX, camera, layer)`, `getTileAtWorldXY(worldX, worldY, nonNull, camera, layer)` ([Tilemap class](https://photonstorm.github.io/phaser3-docs/Phaser.Tilemaps.Tilemap.html)). The page does not say how these behave on hex or iso [unverified].
- Draw order: depth from Y. The official isometric depth-sorting example sets each moving character's and building's depth from its Y, with tiles placed by hand ([depth sorting example](https://phaser.io/oss/examples/v4.1.0/depth%20sorting.md), [devlog 110](https://phaser.io/devlogs/110)).
- Tile occlusion: a forum thread says Y-sorting sprites is not enough for walls a character walks behind; the fix is to split ground and overhead layers ([HTML5 GameDevs](https://www.html5gamedevs.com/topic/27310-wall-tiles-you-can-walk-behind-2x1)).
- Collision on iso: a forum thread reports `convertTilemapLayer` not working for iso collision ([Phaser forum](https://phaser.discourse.group/t/isometric-collision-matter-converttilemaplayer-not-working/11538)). Older forum posts say Phaser has minimal iso support with no physics out of the box ([Phaser forum](https://phaser.discourse.group/t/isometric-support/558)), which may be out of date.
- Infinite maps: no official docs found. Forum users say chunked Tiled exports load without issue ([HTML5 GameDevs](https://www.html5gamedevs.com/topic/38708-support-for-infinite-tiled-maps-in-phaser-3)) and an earlier thread describes an infinite-map decode error. A community splitter tool exists ([npm phaser-map-splitter](https://www.npmjs.org/package/phaser-map-splitter)).
- Pain point: depth sorting with many sprites, including off-screen ones, is slow; a maintainer measured about 20.8 ms per sort on a 200×200 map ([issue #6215](https://github.com/phaserjs/phaser/issues/6215)).
- Navigation on hex/iso: none found [unverified].

---

## 5. Excalibur.js

Sources: [Isometric docs](https://excaliburjs.com/docs/isometric) (version not stated), [excalibur-tiled README](https://github.com/excaliburjs/excalibur-tiled), [Tiled plugin docs](https://excaliburjs.com/docs/tiled-plugin).

- Core `ex.IsometricMap` is a single-layer iso grid built from `IsometricTile` entities, with `worldToTile` and `tileToWorld` helpers ([Isometric docs](https://excaliburjs.com/docs/isometric)).
- Depth sorting: `IsometricEntityComponent` plus an `IsometricEntitySystem` derives z-index from elevation and transform. The exact formula is not given ([Isometric docs](https://excaliburjs.com/docs/isometric)).
- Collision: tiles accept polygon colliders with `addCollider`, and tiles must be `solid` to become fixed colliders ([Isometric docs](https://excaliburjs.com/docs/isometric)). Behaviour for iso-sorted entities is not described.
- Multi-layer maps: stack several IsometricMaps by elevation, or use the Tiled plugin ([Isometric docs](https://excaliburjs.com/docs/isometric)).
- Tiled plugin: orthogonal and isometric rendering. Hexagonal and isometric-staggered are not supported ([excalibur-tiled README](https://github.com/excaliburjs/excalibur-tiled)).
- Infinite maps: sources disagree. The current docs say infinite maps are supported as one Excalibur tilemap per layer ([Tiled plugin docs](https://excaliburjs.com/docs/tiled-plugin)), while the README excerpt does not mention them [unverified].

---

## 6. Love2D: Simple Tiled Implementation (STI)

Sources: [repository README](https://github.com/karai17/Simple-Tiled-Implementation), [docs index](https://karai17.github.io/Simple-Tiled-Implementation/).

- The README recommends LÖVE 11.x and Tiled 1.2.x. Box2D plugin is the one with documented setup (`map:box2d_init(world)`), with a `bump_draw` method for Bump ([README](https://github.com/karai17/Simple-Tiled-Implementation)).
- Orientation support, infinite maps and collision-object generation are not stated on the pages read. The STI docs index was last updated 2020 ([docs index](https://karai17.github.io/Simple-Tiled-Implementation/)).

---

## 7. MonoGame.Extended

Sources: [Tiled docs](https://www.monogameextended.net/docs/features/tiled/), [repository](https://github.com/craftworkgames/MonoGame.Extended).

- `MonoGame.Extended.Tiled` was removed in version 6.0.0 and replaced by a unified tilemap system ([Tiled docs](https://www.monogameextended.net/docs/features/tiled/)). The pages read describe the old namespace.
- Orientations: orthogonal and isometric. Hexagonal and staggered are not supported. Infinite maps are not supported ([Tiled docs](https://www.monogameextended.net/docs/features/tiled/)).
- Tile collision: not described; the object layer types include rectangle, ellipse, polygon, polyline and tile ([Tiled docs](https://www.monogameextended.net/docs/features/tiled/)).
- Run-time editing: `TryGetTile` reads tiles on a `TiledMapTileLayer` ([Tiled docs](https://www.monogameextended.net/docs/features/tiled/)).

---

## 8. GameMaker

Sources: [tilemap_get manual page (LTS)](https://manual.gamemaker.io/lts/en/GameMaker_Language/GML_Reference/Asset_Management/Rooms/Tile_Map_Layers/tilemap_get.htm), [itch.io threads](https://itch.io/post/1338067/view-in-topic).

- The tile-map API documented on the page works with a cell grid. `tilemap_get` takes cell positions from 0 to the map's width and height, and `tilemap_get_at_pixel` takes room positions ([tilemap_get](https://manual.gamemaker.io/lts/en/GameMaker_Language/GML_Reference/Asset_Management/Rooms/Tile_Map_Layers/tilemap_get.htm)).
- No isometric or hexagonal grid is documented in the pages read. Community isometric kits are third-party assets, for example the [isometric tileset](https://marketplace.gamemaker.io/assets/8288/isometric-tileset) on the GameMaker Marketplace.
- Runtime-created layers: `layer_get_tilemap` does not work for layers made at runtime in one reported case ([itch.io](https://itch.io/post/1338067/view-in-topic)).
- Gap: the GameMaker manual's full tile map section could not be read in full, so iso/hex and unbounded-map support is **unverified**.

---

## 9. Defold

Sources: [Tilemap manual](https://defold.com/manuals/tilemap/), [forum: Hexagonal Tilemaps](https://forum.defold.com/t/hexagonal-tilemaps/36087), [forum: Isometric tiled map issue](https://forum.defold.com/t/isometric-tiled-map-issue/11864).

- The manual describes a tilemap component that paints tiles from a Tile Source onto a grid, with `tilemap.get_tile()` and `tilemap.set_tile()` by layer and cell. It does not describe world-to-cell conversion ([Tilemap manual](https://defold.com/manuals/tilemap/)).
- The manual mentions orthogonal, isometric and hexagonal only as Tiled-side options; Defold's own support is not stated ([Tilemap manual](https://defold.com/manuals/tilemap/)).
- Forum replies say the built-in component supports only rectangular tiles. The recommended approach for hex and iso is one game object per tile, as used in Kanji Adventure ([forum: Hexagonal Tilemaps](https://forum.defold.com/t/hexagonal-tilemaps/36087)).
- Tiled iso import ignores the tile height setting for some users, and custom Lua or JSON loaders are used instead ([forum: Isometric tiled map issue](https://forum.defold.com/t/isometric-tiled-map-issue/11864)).
- Infinite maps: not mentioned in the manual page read.

---

## Cross-engine gaps and caveats

- Unity 6 collider behaviour on hex and iso was not confirmed for 6.x; the 2018 manual is the only source.
- Godot hex terrain and infinite maps: not documented in the official pages read.
- Phaser infinite maps and hex or staggered navigation: no official source found.
- GameMaker iso/hex and unbounded maps: unverified from the manual.
- Some third-party issue lists were read through a mirror and their open or closed state was not checked on GitHub, marked above.

---

## What a professional engine is expected to offer

Ranked from the survey by how often the engine docs or the reported pain points point to the item as necessary. "Offered" means the official docs describe it; "partial" means it is limited or only in some variants.

1. **Depth sorting that lets characters interleave with iso or hex tiles.** Godot: per-tile Y Sort Origin plus Y-sort on the layer (offered, with known flicker). Unity: Individual renderer mode and Z-as-Y with a transparency axis (offered; Chunk mode breaks it). Phaser and Excalibur: depth from Y or an IsometricEntityComponent (offered, by hand). Bevy: partial (sorting tweaks per FAQ). Defold, GameMaker: not offered.
2. **Grid shape and layout selected per tileset or per grid, with an offset axis.** Godot (offered: shape, layout, offset axis). Unity (offered: Cell Layout, hex point/flat). bevy_ecs_tilemap (offered: square, hex, iso variants). Phaser (import of iso, hex and staggered from 3.50). Excalibur, MonoGame.Extended: iso only. Defold, GameMaker: rectangular only per the sources.
3. **Coordinate conversion helpers (world to cell, cell to world) that respect the shape.** Godot (offered: `local_to_map`, `map_to_local`, with `to_local` caveat). Unity (`Grid.CellToWorld`, `WorldToCell`, shape-aware). Excalibur (iso `worldToTile`, `tileToWorld`). Phaser (`worldToTileX`, `tileToWorldX`, shape not stated). Defold, GameMaker: not shape-aware in the docs read.
4. **Neighbour queries that respect the shape.** Godot (offered: `get_neighbor_cell`, `get_surrounding_cells` with 6 on hex). Bevy: partial (neighbour helper exists, not described in the sources read). Others: not documented.
5. **Per-tile physics, navigation and occlusion data authored in the tileset.** Godot (offered: physics, navigation and occlusion layers per tile). Unity (partial: Tilemap Collider on cell shape, Composite merge). bevy_ecs_tiled (colliders from tiles, with iso bugs). Phaser, Excalibur (per-tile polygon colliders on iso). Defold: per the tile source collision shapes.
6. **Autotiling and terrain rules on hex and iso.** Unity (offered: Hexagonal and Isometric Rule Tile classes). Godot (terrain sets exist; hex or iso support not documented). Others: not documented.
7. **Chunked rendering with culling and a documented bound on map size.** Godot (quadrants, documented). Unity (Chunk mode, Chunk Culling Bounds). bevy_ecs_tilemap (chunked meshes). Phaser (camera culling, not detailed). Defold, GameMaker: not stated.
8. **Unbounded or infinite maps imported from Tiled.** bevy_ecs_tiled (offered; crash reports). Excalibur (current docs say offered; conflicting README). Unity (bounds grow on write; `CompressBounds` offered; no infinite import). Phaser (forum reports only). Godot, Defold, MonoGame.Extended, GameMaker: not offered per sources.
9. **Tiled import of isometric and hex orientations.** bevy_ecs_tiled (orthogonal, iso, hex; not staggered). Phaser (ortho, iso, hex, staggered from 3.50). Excalibur, MonoGame.Extended: iso only. STI: [unverified]. Defold, GameMaker: not documented.
10. **Run-time editing with per-tile writes and bulk-write options.** Godot (`set_cell`, deferred `changed`, `update_internals`). Unity (`SetTile`, `SetTilesBlock`; bulk writes faster). bevy_ecs_tilemap (one entity per tile, so edits are ECS operations). Defold (`set_tile` on the built-in tilemap).
11. **Pathfinding grid aware of iso or hex.** Godot AStarGrid2D (iso only, offered). All others: none found in the sources.

Items 1, 2, 3 and 8 are where the reported pain is highest. Items 6, 9 and 11 are the thinnest across the field, so they are where a professional engine could differentiate.

---

## Sources (consolidated)

- Godot: https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html ; https://docs.godotengine.org/en/stable/classes/class_tileset.html ; https://docs.godotengine.org/en/stable/classes/class_tilemaplayer.html ; https://docs.godotengine.org/en/stable/classes/class_astargrid2d.html ; https://github.com/godotengine/godot/issues/71257 ; https://github.com/godotengine/godot/issues/54006 ; https://github.com/godotengine/godot/issues/37394 ; https://forum.godotengine.org/t/why-do-you-have-to-parent-a-node-to-a-tilemap-in-order-for-its-ysort-to-work-in-an-isometric-game/25776 ; https://forum.godotengine.org/t/unable-to-properly-y-sort-entities-in-a-multi-leveled-isometric-tilemap/93061 ; https://forum.godotengine.org/t/hex-tilemap-coords-from-mouse-click-event-are-offset/119767 ; https://forum.godotengine.org/t/aligning-tilemap-with-underlying-hex-grid/102169 ; https://forum.godotengine.org/t/performance-issues-of-tilemaplayer-when-using-a-large-tileset/113746 ; https://forum.godotengine.org/t/performance-issues-with-lots-of-tilemaplayers-for-large-3d-stacked-worlds/116503 ; https://forum.godotengine.org/t/avoiding-obstacles-with-tilemaplayer/93475 ; https://store.godotengine.org/asset/zehir/hexagon-tilemaplayer/ ; https://godotengine.org/asset-library/asset/edit/23202
- Unity: https://docs.unity.com/en-us/engine/current/manual/unity2d/tilemaps/reference/grid ; https://docs.unity.com/en-us/engine/6000.6/manual/unity2d/tilemaps/hexagonal-tilemap ; https://docs.unity.com/en-us/engine/6000.6/manual/unity2d/tilemaps/isometric-tilemap/tilemap-renderer-isometric-modes ; https://docs.unity.com/en-us/engine/6000.0/manual/unity2d/tilemaps/reference/tilemap-renderer ; https://docs.unity.com/en-us/engine/current/script-reference/unityengine/tilemaps/tilemap/compressbounds ; https://docs.unity3d.com/Packages/com.unity.2d.tilemap.extras@3.0/api/UnityEngine.html ; https://docs.unity3d.com/es/2018.3/Manual/class-TilemapCollider2D.html ; https://issuetracker.unity3d.com/issues/tilemap-dot-size-does-not-change-when-the-tiles-are-erased ; https://discussions.unity.com/t/isometric-tilemap-sorting-issues/714645 ; https://discussions.unity.com/t/issues-sorting-character-in-z-as-y-isometric-tilemap-issues-sorting-a-character-inside-a-isometric-z-as-y-tilemap/247491 ; https://issuetracker.unity3d.com/issues/isometric-tilemap-incorrectly-sorts-tiles-when-sort-order-is-set-to-bottom-right-or-top-left ; https://issuetracker.unity3d.com/product/unity/issues/guid/974119 ; https://community.gamedev.tv/t/collider-problems/195688 ; https://forum.unity.com/threads/tilemap-settilesblock-performance.537274/ ; https://forum.unity.com/threads/settile-is-too-slow.1278899/
- Bevy: https://github.com/StarArawn/bevy_ecs_tilemap ; https://docs.rs/bevy_ecs_tilemap/latest/bevy_ecs_tilemap/ ; https://docs.rs/bevy_ecs_tilemap/latest/bevy_ecs_tilemap/map/enum.TilemapType.html ; https://github.com/adrien-bon/bevy_ecs_tiled ; https://github.com/adrien-bon/bevy_ecs_tiled/issues/48 ; https://adrien-bon.github.io/bevy_ecs_tiled/FAQ.html
- Phaser: https://photonstorm.github.io/phaser3-docs/Phaser.Tilemaps.Tilemap.html ; https://phaser.io/oss/examples/v4.1.0/depth%20sorting.md ; https://phaser.io/devlogs/110 ; https://github.com/phaserjs/phaser/issues/6215 ; https://www.html5gamedevs.com/topic/27310-wall-tiles-you-can-walk-behind-2x1 ; https://phaser.discourse.group/t/isometric-collision-matter-converttilemaplayer-not-working/11538 ; https://phaser.discourse.group/t/isometric-support/558 ; https://www.html5gamedevs.com/topic/38708-support-for-infinite-tiled-maps-in-phaser-3 ; https://www.npmjs.org/package/phaser-map-splitter
- Excalibur: https://excaliburjs.com/docs/isometric ; https://github.com/excaliburjs/excalibur-tiled ; https://excaliburjs.com/docs/tiled-plugin
- STI: https://github.com/karai17/Simple-Tiled-Implementation ; https://karai17.github.io/Simple-Tiled-Implementation/
- MonoGame.Extended: https://www.monogameextended.net/docs/features/tiled/ ; https://github.com/craftworkgames/MonoGame.Extended
- GameMaker: https://manual.gamemaker.io/lts/en/GameMaker_Language/GML_Reference/Asset_Management/Rooms/Tile_Map_Layers/tilemap_get.htm ; https://itch.io/post/1338067/view-in-topic ; https://marketplace.gamemaker.io/assets/8288/isometric-tileset
- Defold: https://defold.com/manuals/tilemap/ ; https://forum.defold.com/t/hexagonal-tilemaps/36087 ; https://forum.defold.com/t/isometric-tiled-map-issue/11864
