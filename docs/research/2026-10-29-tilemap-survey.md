# Tilemap systems research for Ryo2D (read 2026-10-29)

Method: official docs and release pages fetched via web tools. Where a page did not state something and I am filling in from general knowledge, it is marked "(background knowledge, unverified)". Versions read: Godot docs 4.7 (stable), Unity 6.6 manual (6.3 is LTS), Tiled 1.12.2 (27 May 2026), LDtk 1.5.3 (JSON 1.5.3; search found no newer release; 1.5.3 dated Jan 2024), bevy_ecs_tilemap 0.20.0, bevy_ecs_ldtk 0.16.0 (25 Oct 2026), bevy_ecs_tiled 0.15.0, tiled crate 0.16.0, bevy_fast_tilemap 0.8.1 (Bevy 0.14, stale).

---

## 1. Godot 4.x (docs 4.7)

Sources: https://docs.godotengine.org/en/stable/classes/class_tilemaplayer.html , https://docs.godotengine.org/en/stable/tutorials/2d/using_tilesets.html , https://docs.godotengine.org/en/stable/tutorials/2d/using_tilemaps.html , https://docs.godotengine.org/en/stable/classes/class_tiledata.html , https://docs.godotengine.org/en/stable/classes/class_tilesetatlassource.html

- **Node model.** One `TileMapLayer` node per layer, each with a `TileSet` resource (shareable). The old multi-layer `TileMap` node is deprecated. Layers are reordered and toggled in the scene tree; deleting a layer deletes its tiles.
- **TileSet sources.**
  - Atlas sources: a tilesheet with uniform tile size, with margins, separation, texture region size, and `use_texture_padding` (default true; generates an internal 1px-padded texture to prevent seams between tiles, at a memory/CPU cost whenever the atlas changes). Tile shapes: square, rectangle, hexagon, isometric. Dragging a sheet in auto-creates tiles and skips fully transparent cells. An Atlas Merging Tool combines atlases and keeps "tile proxies" mapping old IDs to new.
  - Scene collection: a scene is a tile. Each placed scene tile is a separate instance, so the docs warn of "greater performance overhead"; use for shops, ambient audio, particles.
- **Per-tile layers declared on the TileSet.** Physics layers (collision polygons, per-polygon one-way flag), navigation layers, occlusion layers (light occluders), and custom data layers (typed, named, e.g. `damage_per_second`; reorderable without data loss). Rendering per tile: modulate, material, z_index, texture_origin, y_sort_origin, probability. Polygon editor: press F for a default rectangle; paint a shape across many tiles.
- **Terrains (autotiling).** Terrain sets with match mode (Corners and Sides / Corners / Sides); each tile gets terrain_set + terrain id and peering bits. API: `set_cells_terrain_connect()` (joins neighbours, may rewrite adjacent tiles) and `set_cells_terrain_path()` (joins along an ordered path). The TileSet must define all needed combinations or results are unexpected.
- **Alternative tiles and flips.** An alternative tile is a variant of a base tile (flip H/V, transpose, modulate, material, z, offsets). It does NOT inherit the base tile's physics/custom data; they must be re-set. Flip H/V + transpose combine to give 90/180/270 rotations. Since 4.2, the TileMap editor toolbar can rotate/flip placed tiles without creating alternatives.
- **Animated tiles.** Atlas-source animation: columns, frame count, per-frame duration, speed, separation; mode `DEFAULT` (all tiles start together, identical) or `RANDOM_START_TIMES`. Frames live in the atlas as adjacent cells.
- **Patterns.** `TileMapPattern` (from `get_pattern()`, pasted with `set_pattern()`, `map_pattern()` for half-offset shapes). Patterns are stored in the TileSet resource, so they are shared across layers, and are saved in editor via copy into the Patterns tab.
- **Y-sorting.** Layer-level `y_sort_origin`, `x_draw_order_reversed`; per-tile y_sort_origin. Enabling y-sort makes rendering group by Y rather than by quadrant (effectively per-row/per-tile cost).
- **Rendering and physics chunking.** `rendering_quadrant_size` (default 16 -> 16x16 = 256 cells per canvas item; ignored when y-sorted). `physics_quadrant_size` (default 16) merges collision shapes of similar cells per quadrant (set to 1 for exact per-cell body coords). `use_kinematic_bodies` for moving platforms. Navigation and occlusion toggles per layer; recommended to bake navigation to a region for performance.
- **Runtime.** Edits are batched to end of frame; `update_internals()` forces update but is expensive; `_use_tile_data_runtime_update()` / `_tile_data_runtime_update()` give per-tile runtime overrides. Serialized coordinates limited to signed 16-bit (-32768..32767).
- **Editor workflow.** Modes: Selection, Paint, Line, Rectangle, Bucket Fill (contiguous option), Picker, Eraser; random placement and a "scattering" probability; Terrain tab with Connect and Path modes; multi-select and property painting in the TileSet editor; patterns tab.
- **Scale/perf anecdotes (forums, not official):** a 700x700 layer reported 4-5 ms for a single `set_cell` on 4.5 (https://forum.godotengine.org/t/large-2d-tilemap-causes-4-5ms-lag-on-setcell-invocation/126593); advice to raise rendering quadrant to 32-64 for big outdoor maps (blog, secondary); lowering quadrant or y-sort makes every change costlier.

## 2. Unity 2D Tilemap (Unity 6.6 manual; 6.3 LTS)

Sources: https://docs.unity.com/en-us/engine/current/manual/unity2d/tilemaps , https://docs.unity.com/en-us/engine/current/script-reference/unityengine/tilemaps/tilemaprenderer/mode , https://docs.unity.com/en-us/engine/current/script-reference/unityengine/tilemaps/tilemapcollider2d , https://github.com/Unity-Technologies/2d-extras

- **Structure.** A `Grid` GameObject defines cell layout (rectangular, hexagonal flat/pointy-top, isometric / isometric Z-as-Y) and child `Tilemap` GameObjects each hold a layer. Manual sections: introduction/layouts, Tile Palettes, painting, brush picks, collision, hexagonal and isometric tilemaps, custom tiles and brushes, reference.
- **Tile Palette** window: palette assets made from sprites/textures, with Brushes; "brush picks" save a tile/group plus brush settings for reuse.
- **2D Tilemap Extras** (package; the old GitHub repo is going read-only): Rule Tile (terrain/pipeline/random/animated behaviours), Hexagonal and Isometric Rule Tile, RuleOverrideTile, Animated Tile (sprite list), Pipeline, Terrain, Random, Weighted Random tiles. Brushes: Coordinate, Line, Random, Prefab, PrefabRandom, GameObject, TintBrush (per-cell tint), TintBrushSmooth (needs custom shader), Group.
- **TilemapRenderer modes.** `Chunk` batches sprites into grouped chunks (the performance mode); `Individual` renders each sprite separately (needed when tiles must sort/interleave with other objects, e.g. isometric depth); `SRPBatch` sends batchable chunks to the SRP Batcher. Chunk size/culling details are not on the pages I read (background knowledge: chunk mode culls per chunk; unverified).
- **Collision.** `TilemapCollider2D` creates per-tile shapes from sprite physics shape or grid cell; set "Used By Composite" with a `CompositeCollider2D` (on a static Rigidbody2D) to merge tiles into outlines (background knowledge on the setup; page confirmed only the following): `extrusionFactor` "eliminates fine gaps between Tiles when using a CompositeCollider2D", `useDelaunayMesh` extra triangulation step, `maximumTileChangeCount` (threshold of accumulated changes before full rebuild instead of incremental), `hasTilemapChanges` / `ProcessTilemapChanges()` to apply edits immediately.
- **Known gap:** no built-in per-tile one-way behaviour; forum workaround is separate objects (https://discussions.unity.com/t/tilemap-one-way-platform-stuck-in-between-and-falling-through/838902, forum post, not official).

## 3. Tiled (1.12.2, 27 May 2026)

Sources: https://www.mapeditor.org/news , https://doc.mapeditor.org/en/stable/reference/tmx-map-format/ , https://doc.mapeditor.org/en/stable/reference/json-map-format/ , https://doc.mapeditor.org/en/stable/reference/global-tile-ids/ , https://doc.mapeditor.org/en/stable/manual/worlds/ , https://doc.mapeditor.org/en/stable/manual/automapping/ , https://doc.mapeditor.org/en/stable/manual/objects/ , https://doc.mapeditor.org/en/stable/manual/editing-tilesets/

- **Releases.** 1.12 (13 Mar 2026): rewritten Properties view, list-typed custom properties, new Oblique map orientation, layer blending modes, capsule objects, ellipse object; 1.12.1 (25 Mar), 1.12.2 (27 May). Previously 1.11.2 (Jan 2025).
- **Formats.** TMX (XML) and TMJ (JSON; field names differ slightly). External tilesets TSX/TSJ via `source` + `firstgid` (local id = gid - firstgid). Template files (.tx/.tj) with one object; instance properties override template. Tile layer data: CSV, base64, with gzip/zlib/zstd compression; little-endian u32 GIDs. TMJ `version` is a string since 1.6.
- **GID flags** (top bits of the u32): 0x80000000 horizontal flip, 0x40000000 vertical flip, 0x20000000 diagonal (anti-diagonal) flip in ortho/iso, **60 degree clockwise rotation in hex maps**; 0x10000000 ignored in ortho/iso but **120 degree rotation in hex**. Must clear all four before looking up the tile (including bit 29 in non-hex maps). Ortho/iso apply diagonal flip first, then H, then V.
- **Infinite maps.** `infinite=1`; tile layer data stored in `<chunk>` elements (x, y, width, height; negative coordinates allowed; default chunk size 16x16 set in editor settings). JSON layers carry `startx/starty`.
- **Layer types.** Tile, object group, image layer (repeatx/repeaty since 1.8), group layer (nesting). Layers render in document order. **Parallax:** per-layer `parallaxx/parallaxy` (since 1.5), map `parallaxoriginx/y` (since 1.8). Also offset, tint, opacity.
- **Objects.** Rectangle, point, ellipse (1.12), capsule (1.12), polygon, polyline, text, tile objects (alignment from tileset Object Alignment). Object-reference properties (arrows in editor). Insert Template tool.
- **Custom properties.** string, int, float, bool, color, file, object, class (nested members; `propertytype` names the custom type since 1.8); `class` attribute on many elements since 1.9; list-typed in 1.12. Custom enum/class types defined at project level.
- **Tilesets.** Atlas or image-collection tilesets (per-tile images, variable sizes). Tile collision editor (shapes with properties; can be shown on map since 1.3), per-tile animation (one looping animation per tile, frames reference other tiles, 100 ms default), per-tile class/probability. **Wang sets** (up to 254 colours, `wangid` as comma-separated colour indexes per tile; legacy "terrain types" deprecated). Corner/edge/mixed sets.
- **Automapping.** Rule maps with `input`/`output` layers and a rules.txt; special tiles Empty/Ignore/NonEmpty/Other/Negate; options MatchInOrder, AutomappingRadius, per-rule ModX/ModY/Offset/Probability/Disabled, flip-ignoring options; 1.9 rewrite 10-30x faster; "AutoMap While Drawing".
- **World files.** JSON `.world`: explicit `maps` with x,y or regexp `patterns` with multipliers; `onlyShowAdjacentMaps` to load just neighbours; world auto-reloads on disk change; World Tool (1.4+) to place maps.

## 4. LDtk (1.5.3)

Sources: https://ldtk.io/ , https://ldtk.io/json/ , https://ldtk.io/docs/game-dev/json-overview/ , https://ldtk.io/docs/game-dev/json-overview/entity-fields/ , https://ldtk.io/docs/game-dev/json-overview/optional-separate-levels/ , https://ldtk.io/docs/game-dev/json-overview/layer-instances/ , https://ldtk.io/docs/general/world/ , https://ldtk.io/docs/game-dev/super-simple-export/ , https://ldtk.io/docs/general/auto-layers/ , https://ldtk.io/docs/game-dev/json-overview/world-layout/

- **Who/why.** Made by Sebastien Benard (Deepnight; ex Dead Cells director). Free, open source, aimed at platformers/top-down (not iso 3D). Indie appeal (from the site): fast purpose-built UI, strong auto-tiling rules, typed entity fields, Aseprite live reload, backups after crash, ready loaders for Unity, Godot, Haxe, Playdate, MonoGame, GameMaker, Bevy; documented JSON plus quicktype schemas; also exports Tiled TMX.
- **Worlds and levels.** A project holds levels placed in a world with layout `GridVania` (snaps to a world grid), `Free`, `LinearHorizontal`, `LinearVertical`; multi-worlds is a project flag (schema lists `MultiWorlds`; structure not explained on pages I read). Each level has world coordinates and a `__neighbours` array: `{levelIid, dir}` where dir is n/s/e/w, plus `<`/`>` (neighbour depth lower/higher) and `o` (overlap, same depth); corner-only `nw/ne/sw/se` added in 1.5.3. Neighbours are computed by the editor, so games get adjacency for free.
- **Definitions vs instances.** `defs` holds layer, entity, tileset, enum, externalEnum, level-field definitions; level `layerInstances` hold data. Layer instance arrays: `intGrid`, `autoTiles`, `gridTiles`, `entityInstances` (only the one matching the layer type is filled).
- **IntGrid.** Integer grid with per-value colour and optional identifier; the usual source for collision, terrain type, and auto-layer input.
- **Auto-layers.** Either an IntGrid layer with tileset + rules, or a "pure" auto-layer holding rules over another IntGrid layer. Rules are grid patterns; "resolved internally by the editor", and the JSON stores the resulting `autoTiles` so games need not implement rules.
- **Entities.** Placed on entity layers; custom fields (Int/Float/Bool/String/Color/Enum/Array of these, with min/max and constraints; also entity references, points, tiles per the editor - the JSON page I read covered the first four kinds, rest from background knowledge). Field values appear in `fieldInstances[].__value` with `__type`. Local enums plus external enum files (`relPath`).
- **External levels.** Option: single `.ldtk` file, or levels split into `<project>/<level>.ldtkl` files (main file then omits layer content). Good for diffs and lazy loading.
- **Simplified export** ("Super Simple" option): per level folder with `_composite.png`, one PNG per layer, an IntGrid PNG (1 pixel per cell) and CSV per IntGrid layer, and optional `data.json` with entities.
- **Caveat:** no LDtk release newer than 1.5.3 was found; its last release is old relative to Oct 2026, though loaders still track it (bevy_ecs_ldtk lists LDtk 1.5.3).

## 5. Rust and Bevy ecosystem

- **bevy_ecs_tilemap 0.20.0** (Bevy 0.20; MIT) - https://docs.rs/bevy_ecs_tilemap/latest/bevy_ecs_tilemap/ , https://github.com/StarArawn/bevy_ecs_tilemap . One entity per tile (queryable with ordinary systems); tiles grouped into chunks, one mesh per chunk (`TilemapRenderSettings { render_chunk_size: UVec2, y_sort: bool }`: bigger chunks for static maps, smaller for frequently changing ones); layers via multiple tilemap entities with z separation of >= 1.0; sparse maps; GPU-driven animations; ortho/iso/hex; texture-array support; anchoring; basic Tiled import. Trade-offs: per-tile entities cost memory and spawn time on huge maps (the repo's `bench` example "takes a while to load"; README says release 0.15 made it about 3x faster, 60 to 180 fps); the GitHub release page shown to me was stale (v0.15.0, Dec 2024) while docs.rs shows 0.20.0, so release metadata is inconsistent; ~1.3k stars, 67 open issues.
- **bevy_ecs_ldtk 0.16.0** (25 Oct 2026; Bevy 0.20, bevy_ecs_tilemap 0.20, LDtk 1.5.3) - https://docs.rs/bevy_ecs_ldtk/latest/bevy_ecs_ldtk/ , https://github.com/Trouv/bevy_ecs_ldtk . Features: `internal_levels` / `external_levels`, `derive` (`LdtkEntity`, `LdtkIntCell` macros to spawn bundles for entities and IntGrid values), `render` (disable for headless), `atlas` (WASM and tile spacing), `scene` (WIP). `EntityIid` on entities, `IntGridCell` component, worldly entities, level loading/neighbour handling, hot reload (README claims; the docs page does not describe it). Trade-off: tied to Bevy ECS + bevy_ecs_tilemap versions; `scene` registration incomplete.
- **bevy_ecs_tiled 0.15.0** (Bevy 0.20) - https://docs.rs/bevy_ecs_tiled/latest/bevy_ecs_tiled/ . Ortho/iso/hex, finite and infinite, embedded/external tilesets, animated tiles, image layers, tile objects, Tiled worlds; entity hierarchy map > layer > tile/object; custom properties become components; auto colliders through Rapier or Avian; hot reload from Tiled.
- **tiled crate (rs-tiled) 0.16.0** - https://github.com/mapeditor/rs-tiled . Engine-agnostic TMX/TSX reader, `ResourceReader` abstraction (embed, custom FS, WASM needs a custom reader, no async loading), min TMX 0.13; 406 commits, 11 open issues. Parser only, no rendering.
- **bevy_fast_tilemap 0.8.1** (depends on Bevy 0.14, so stale) - https://docs.rs/bevy_fast_tilemap/latest/bevy_fast_tilemap/ . One quad per layer, shader selects the tile per pixel; "hundreds of fps, largely independent of map size"; ortho and axonometric; overlap by dominance or perspective. Trade-off: map data must live in a GPU texture; per-tile entities/ECS access not available; unmaintained against current Bevy.
- ldtk_rust: not verified in this pass (I did not fetch it); treat as a thin generated-schema loader (background knowledge, unverified).

## 6. Rendering and performance techniques

Sources: https://docs.rs/bevy_ecs_tilemap/latest/bevy_ecs_tilemap/map/struct.TilemapRenderSettings.html , https://docs.rs/bevy_fast_tilemap/ , https://jvm-gaming.org/t/the-overly-complicated-tile-rendering-of-robot-farm/58733 , Godot/Unity pages above.

- **Chunked meshes (Godot quadrants 16x16 by default, Unity Chunk mode, bevy_ecs_tilemap render chunks).** Tiles grouped so a change rebuilds only one chunk and culling is per chunk; draw calls = visible chunks. Chunk size trades rebuild cost against draw calls: bevy_ecs_tilemap says bigger for static, smaller for dynamic; Godot community suggests 32-64 for large static maps.
- **Whole layer in one shader pass.** One quad per layer, a tile-index texture (e.g. RGBA16UI -> 65,536 tile ids, per the Robot Farm write-up) plus tileset sampling in the fragment shader. Cost scales with screen pixels, not map size (bevy_fast_tilemap's claim); edits are cheap texture writes; animation is a shader time offset. Weakness: per-tile effects (y-sort among objects, per-tile entities) need extra work; very large maps need a sparse/paged index texture.
- **Texture arrays vs atlases.** Atlas sampling bleeds across tile edges with filtering/mipmaps/subpixel camera; fixes are edge padding/extrusion (Godot's `use_texture_padding` generates a 1px padded copy), nearest sampling with half-texel inset, or a 2D texture array with one tile per layer (no bleeding by construction, but requires equal tile sizes and a layer limit; bevy_ecs_tilemap offers texture arrays). The Robot Farm write-up stores the tileset as a texture array for this reason.
- **Culling.** Chunk-level frustum culling (all engines). Tiled's `onlyShowAdjacentMaps` and LDtk's per-level files show the same idea at the map/level scale (streaming).
- **Animated tiles without rebuilds.** Keep animation out of vertex data: Godot's tileset animation frames are adjacent atlas cells with a shared clock (or random start times); bevy_ecs_tilemap does GPU-driven animation (start frame, end frame, speed per tile, resolved in shader); Tiled stores per-tile frame lists. Pattern: store a base tile index plus an animation id, resolve frame = f(time) in the shader.
- **Map sizes.** No official numbers found. Reported: Godot 700x700 layer works but a single `set_cell` costs ms; Godot 16-bit coordinate limit (+-32767 cells); Tiled/LDtk chunk or level the world. I did not find authoritative benchmark numbers for Unity.
- **Y-sort cost.** Godot disables quadrant batching when y-sorted; bevy_ecs_tilemap `y_sort` sorts by chunk z/y; Unity needs Individual mode for per-tile sorting against other objects.

## 7. Collision from tiles

- **Merging.** Godot `physics_quadrant_size` merges similar cells per quadrant; Unity `CompositeCollider2D` merges tile shapes into outlines (use `extrusionFactor` to close seams); greedy rectangle merge (horizontal runs then vertical, or try both orders and keep the fewer-rectangle result) is the common DIY approach (https://emanueleferonato.com/?p=15637, https://docs.flatredball.com/flatredball/tiled-plugin/glue-gluevault-component-pages-tile-graphics-plugin-tileshapecollection/addmergedcollisionfromtileswithproperty). bevy_ecs_tiled auto-generates colliders for Rapier/Avian. Keep one-way and slope tiles out of the merge pass. Merged shapes lose per-cell identity (Godot: physics_quadrant_size = 1 to get exact cell from a body).
- **One-way tiles.** Godot: per-polygon "one way" checkbox on tile collision polygons (tunnelling possible if the margin is smaller than movement per frame). Unity: no built-in per-tile one-way (workaround objects). Tiled: via custom property on the tile/collision object that your engine interprets. LDtk: via IntGrid value or entity.
- **Slopes.** Authored as per-tile polygons in Godot (TileSet physics editor) and Tiled (tile collision editor, polygons); Unity from sprite physics shape. One-way slopes are awkward: forum reports in Godot 3.2 (https://forum.godotengine.org/t/no-way-to-make-tilemap-one-way-collision-slope-in-3-2/13674, old). Many platformers treat slopes as height-profile tiles instead of polygons.
- **Per-tile shapes authored in editor.** Godot (physics layers + polygon editor, multi-tile painting), Tiled (collision editor, shapes can carry properties), Unity (sprite physics shape / Tile Palette), LDtk (none: it uses IntGrid cells, so shapes are an engine-side convention).

## 8. Authoring loop designers expect

- **Hot reload.** bevy_ecs_tiled: edits in Tiled appear in the running game without recompiling; bevy_ecs_ldtk lists hot reloading as a feature; Tiled world files reload on disk change inside the editor; LDtk live-reloads Aseprite files inside the editor; Godot and Unity reimport on file change as normal assets. Designers expect: save in editor -> map in running game updates within a second, camera and player state kept.
- **Entity/object placement.** Tiled: object layers with typed objects, tile objects, templates, object references. LDtk: entity layers, typed fields, refs, enums, per-level fields. Godot: scenes as tiles or nodes alongside layers. Unity: Prefab/GameObject brushes.
- **Custom data.** Godot custom data layers per tile; Tiled custom properties and class types on map/layer/tile/object; LDtk entity fields, level fields, enums, IntGrid identifiers; Unity ScriptableObject tiles/Rule Tile subclasses.
- **Prefab/semantic bridges.** Component-from-properties (bevy_ecs_tiled), derive-macro bundle spawn for LDtk entities (bevy_ecs_ldtk), Godot importer plugins for Tiled/LDtk.

---

## What every professional tilemap system has

- Multiple stacked layers with per-layer visibility, offset, opacity, and draw order (Godot, Unity, Tiled, LDtk, bevy_ecs_tilemap).
- Tile atlas/tileset assets with stable tile ids, separate from the map data (all).
- Per-tile flip/rotate flags (Godot flip+transpose, Tiled GID bits incl. hex rotation, Unity tile transform).
- Auto-tiling / terrain rules (Godot terrains, Unity Rule Tile, Tiled Wang sets + automapping, LDtk auto-layer rules).
- Animated tiles driven from tileset data (Godot, Unity Animated Tile, Tiled tile animation, bevy_ecs_tilemap GPU animation; LDtk via engine).
- Custom per-tile data/properties readable at runtime (Godot custom data layers, Tiled properties/classes, LDtk IntGrid identifiers, Unity tile subclasses).
- Per-tile collision shapes or a grid collision mask that the physics layer can consume (Godot, Unity, Tiled collision editor, LDtk IntGrid).
- Chunked or batched rendering with culling (Godot quadrants, Unity Chunk mode, bevy_ecs_tilemap chunks, bevy_fast_tilemap single-quad).
- Object/entity layer with typed custom fields for spawn points, triggers, and enemies (Tiled objects, LDtk entities; Godot/Unity via scenes/prefabs).
- Multiple map shapes: orthogonal, isometric, hexagonal (Godot, Unity, Tiled, bevy_ecs_tilemap; LDtk ortho only).
- Stamp/pattern, rectangle, line, fill, and eraser editing tools (Godot, Unity brushes, Tiled, LDtk).
- A text format that merges cleanly in version control (Tiled TMX/TMJ, LDtk JSON + .ldtkl, Godot .tres/.tscn, Unity YAML).

## Nice extras some have

- Hex rotation bits in the GID (Tiled; Tiled automapping can ignore hex rotate).
- Infinite maps stored as chunks with negative coordinates (Tiled; Godot sparse cells within +-32767).
- World files and level neighbour data for multi-room worlds (Tiled .world, LDtk GridVania/Free/Linear + `__neighbours`).
- Separate per-level files for lazy loading and diffs (LDtk `.ldtkl`; Tiled worlds with `onlyShowAdjacentMaps`).
- Parallax and image layers (Tiled parallax, image layer repeat; LDtk level background images).
- Object templates and object-reference properties (Tiled).
- Custom property classes/enums defined once and reused (Tiled, LDtk enums).
- Scene/prefab tiles (Godot scene collection, Unity Prefab brushes; bevy_ecs_ldtk entity bundles).
- Per-tile y-sort origin and z-index (Godot).
- Random tile with weights and scatter (Godot, Unity Weighted Random, Tiled probability).
- Randomized animation start times (Godot).
- Atlas merge tool with tile id proxies (Godot).
- Collision merging into composite outlines (Unity CompositeCollider2D, Godot physics quadrants, DIY greedy merge).
- Per-tile one-way collision flag (Godot).
- Editor-side rule resolution baked into data so games need no rule engine (LDtk `autoTiles`; Tiled automapping runs in editor).
- Simple export for quick engines: PNG per layer + IntGrid CSV (LDtk).
- GPU-only one-quad-per-layer rendering independent of map size (bevy_fast_tilemap).
- Per-tile ECS entities for queries and mutation (bevy_ecs_tilemap, bevy_ecs_tiled).
- Live hot reload of editor edits into the running game (bevy_ecs_tiled, bevy_ecs_ldtk).
- Per-tile tint brushes (Unity TintBrush/TintBrushSmooth).
- Oblique map orientation and layer blend modes (Tiled 1.12).
- Tile runtime data overrides (Godot `_tile_data_runtime_update`).

## Gaps and caveats in this research

- Unity Tilemap pages for chunk size and culling, and TilemapCollider2D composite specifics, were thin; general-knowledge statements are marked.
- LDtk multi-worlds JSON structure and ldtk_rust were not verified.
- No authoritative map-size benchmarks found for any engine; only forum anecdotes.
- bevy_ecs_tilemap release metadata conflicts between GitHub (v0.15.0 shown) and docs.rs (0.20.0).
