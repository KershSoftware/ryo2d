# Ryo2D tilemap support today (read-only recon, 2026-10-29, main at f415486b)

Paths are relative to /Users/mbreckon/Projects/ryo2d. "(inferred)" marks conclusions not read directly.

## 1. What exists

### Size
`src/core/tilemap/` is 3,524 lines in 9 files: tiled.rs 1384 (about 560 production, rest tests), collision.rs 482, manager.rs 461, map.rs 366, autotile.rs 293, tileset.rs 236, chunk.rs 142, layer.rs 136, mod.rs 24, plus AGENTS.md. Of the lines, `tiled.rs` is mostly tests (29 tests from line 570).

### Data model (all plain structs, not ECS)
- `TileId = u32` (tileset.rs:4). A layer cell is `Option<TileId>`: no flip, rotation or per-cell tileset bits (layer.rs:12).
- `Tileset` (tileset.rs:12-35): name, `image_path: String` (never loaded), tile size, columns, tile_count, spacing, margin, image size, `properties: HashMap<TileId, TileProperties>`. `tile_uv()` (tileset.rs:70) computes UVs; nothing outside tests calls it.
- `TileProperties` (tileset.rs:41): `solid`, `platform`, `animation: Option<TileAnimation>` (frames of `(TileId, ms)`), `custom: HashMap<String,String>`. `get_properties` clones the struct, including the custom map, on every call (tileset.rs:90).
- `TilemapLayer` (layer.rs:8): name, flat `Vec<Option<TileId>>`, width/height, ONE `tileset_index` per layer, visible, opacity, `z_order: i32`, parallax_x/y. No layer offset, tint, or kind.
- `Tilemap` (map.rs:27): `tilesets: Vec<Tileset>`, `layers`, tile size, map size, `orientation`. Origin is fixed at (0,0), top-left, +Y down. Fields are all public.
- `TilemapManager` (manager.rs:36), a resource at `ctx.tilemaps()` (scene/context.rs:233, inserted at engine/mod.rs:409). It is a `HashMap<Entity, TilemapRecord{tilemap, chunks}>`. The tilemap entity itself carries no components. The only engine system is `forget_despawned_tilemaps` (engine/mod.rs:500 in `Last`, and scene/cleanup.rs:26).

### Loading
- `load_tiled_map(path: &str) -> Result<(Tilemap, Vec<TiledObject>), TilemapError>` (tiled.rs:447). It uses `std::fs::read_to_string` on the raw path, so it is relative to the working directory, not the asset root. It does not use the asset registry, and it cannot work on web (inferred: no fs on wasm; asset AGENTS.md says `DiskSource::read` is Unsupported there).
- Formats: JSON only, `.tmj` plus external `.tsj` resolved against the map's directory (tiled.rs:481-495). Inline tilesets also work. There is no TMX/XML (AGENTS.md "JSON-only" decision). Infinite maps are rejected (tiled.rs:464).
- Orientation: "orthogonal", "isometric" and "hexagonal" parse (tiled.rs:293-304). Hex always becomes `HexagonalFlat`; `staggeraxis` is ignored. `TilemapOrientation::HexagonalPointy` is never produced. The coordinate math ignores orientation.
- Layers: `tilelayer` and `objectgroup` are handled. Everything else (imagelayer, group) is silently skipped (tiled.rs:~551).
- Tile layer `data` is `Vec<u32>` (tiled.rs:213), so only the uncompressed array form works. A `"encoding":"base64"` layer (string data) fails with a Parse error.
- Objects are flattened into one `Vec<TiledObject>` with no layer name (tiled.rs:~543). Fields: name, obj_type (`"type"`), x/y/w/h, rotation, shape (Rect, Ellipse, Point, Polygon, Polyline), properties (Bool/Int/Float/String only).
- Tile properties: `solid` and `platform` are read only if the JSON value is a bool (tiled.rs:~359). Other properties become strings in `custom`. Animation frames are parsed.
- Tileset image paths are stored as written, not rebased against the tsj or map directory.

### Drawing (and its cost)
- There is no tilemap renderer. `src/core/tilemap/AGENTS.md` says "Rendering is a data stub"; nothing in `src/core/render`, `graphics` or `engine` references `Tilemap`/`TilemapManager` (grep: only engine/mod.rs:409, :500, scene/context.rs, scene/cleanup.rs).
- The only tile drawing is `CompositeTile`/`TileDirection`/`TilePart` (`src/core/graphics/tile.rs`, 572 lines). It is unrelated to tilemaps: it repeats sprites along one axis, for pipes in flappy. It is laid out in `render/entities.rs` (Tiled run at :670, composite at :735).
- Cost model of the only available route (the demo's per-tile `Renderable` entities): one ECS entity per tile. There is "no render cache" (render/AGENTS.md:53), so every frame the entity query walks them, interpolates positions, sorts them (z, kind, submission) and emits one 128-byte `SpriteInstance` quad per tile into the batch's instanced stream (render/AGENTS.md:70). A 200x200 map is 40,000 entities, 5 MB of instances per frame, all re-sorted every frame (inferred from the cited design).
- The batch merges consecutive items with equal `DrawState` into draws (render/AGENTS.md:71), so draw calls would stay low, but the CPU cost scales with tile count and not with what is visible.
- Chunk machinery exists but is unused: `CHUNK_SIZE = 16`, `ChunkCoord`, `ChunkState{dirty, has_tiles}`, `chunk_visible`, `TilemapManager::visible_chunks` (manager.rs:162). Nothing consumes `dirty` or `visible_chunks` outside tilemap tests.

### Collision
- Two unrelated paths:
  1. `generate_collision_rects[_region]` (collision.rs:41, :54): greedy-merged `CollisionRect`s in tile coordinates from `solid` only. It ignores platform and slope, it never produces colliders, and `build_solid_bitmap` ignores a tile's layer-specific tileset, using `layer.tileset_index` (collision.rs:~97).
  2. `Tilemap::tile_colliders() -> TileColliders` (map.rs:158): reads `solid`, `platform` and a `slope` custom property. Ranking is Solid > Slope > OneWay across layers (the physics-gameplay spec says "OR across layers"; the code is current). Over 8192x8192 cells it logs and gives an empty grid.
- `TileColliders` (src/core/physics/tiles.rs, 946 lines, 24 tests) is a component requiring `Transform`. It holds a private grid of `TileCell {Empty, Solid, OneWay, Slope(SlopeCorner)}` plus layer/mask/friction/restitution/`solid_rects`. Its `generation` counter drives a full rebuild of all child collider entities before the next physics step (`rebuild_tile_colliders`, FixedSet::Movement). It builds seamless polyline outlines (or merged rects), 45-degree slope polygons and one-way segments. Rebuilds are all-or-nothing ("incremental tile rebuilds" are out of scope in the spec).
- Relationship: the physics module defines `TileColliders` and the tilemap module depends on physics (map.rs:5), not the reverse (spec physics-gameplay-design.md:36). `TileColliders` is a snapshot copy. Editing `TilemapManager::set_tile` does not update an existing `TileColliders` (manager.rs:97-127 touches only chunk flags) (read).
- `NavigationGrid::block_tiles(&TileColliders, origin)` also consumes the grid (navigation/AGENTS.md; physics AGENTS.md:128).

### Autotile
`AutoTileSet{name, terrain_id, mode, default_tile, rules: HashMap<u8,TileId>}` with `add_rule`/`resolve`, and `compute_bitmask_minimal` (4-bit) / `compute_bitmask_full` (8-bit with diagonal reduction) take an `is_same` closure (autotile.rs). It is pure helpers with 9 tests. No code applies it: no call sites in src, examples or tests (grep). There is no on-set neighbour update, no Wang/terrain import, and `terrain_id` is unused.

### Chunking
See Drawing above: constants, coordinates, a dirty flag and a visibility test, with no consumer. `has_tiles` is set true on set_tile but never reset when a tile is cleared (manager.rs:~120). `dirty` is never cleared (nothing builds meshes).

### Editing in code
- On the data: `TilemapLayer::set_tile` (layer.rs:71, returns previous), `TilemapManager::set_tile(entity, layer, x, y, Option<TileId>)` (marks the chunk dirty), `Tilemap::add_layer`/`add_tileset`. `Tilemap` is `Clone` and all fields are public, so a game can build maps procedurally (the demo does).
- No batch edit, fill, flood, or undo. No edit event or message. No world-to-tile conversion with a map offset or transform.

### Animation
`TileAnimation` data is parsed from Tiled (tiled.rs animation parse) and stored. Nothing advances it or reads it at draw time (no renderer).

### Layers and z-order
Each layer has `z_order`, `visible`, `opacity`, `parallax_x/y`. The Tiled loader assigns z_order sequentially across all layers including object and skipped layers (tiled.rs:~535-560). None of it is consumed (no renderer). `visible_chunks` honours `visible` only.

### Objects and entities from maps
`TiledObject` is parsed, but nothing spawns entities from it. There are no tile-object (`gid`) fields, no collision shapes attached to tiles, and no `class` field.

### Hot reload and asset integration
- None. `Tilemap`/`Tileset` are not registry assets. The asset registry registers Image, Shader, Font, BitmapFont, UiFile, Theme, AudioClip, Sound, Music, Mixer only (asset/AGENTS.md:23).
- The registry design explicitly defers it: "Not tilemaps (their loader is rewritten later)" (docs/specs/2026-10-23-asset-registry-design.md:20, :149, :214). follow-ups.md:87 says `load_tiled_map` resolves against the working directory "until 5b and the tilemap work".
- Tileset images are plain strings, never `Handle<Image>`, so there is no dependency tracking and no reload.

### Public surface
Prelude exports (lib.rs:210-216): AutoTileMode, AutoTileSet, ChunkCoord, ChunkState, CollisionRect, TileAnimation, TileId, TileProperties, TiledObject, TiledShape, TiledValue, Tilemap, TilemapError, TilemapLayer, TilemapManager, TilemapOrientation, Tileset, compute_bitmask_*, generate_collision_rects[_region], load_tiled_map. `TileColliders`, `TileCell`, `SlopeCorner` come from the physics export block.

## 2. Verified bugs and gaps (audit claims checked)

| # | Claim / finding | Status | Evidence |
|---|---|---|---|
| 1 | Multi-tileset layers take the last tile's tileset | STILL HOLDS | `layer.tileset_index = ts_idx;` runs inside the per-tile loop (tiled.rs:534), overwriting each time. The `multiple_tilesets_gid_resolution` test (tiled.rs:1287) passes because it only checks per-test layers; a layer mixing tilesets gets local ids interpreted against the wrong tileset for `get_properties`, `is_solid_at`, `tile_colliders` and collision rects. |
| 2 | Flip/rotation GID bits not masked | STILL HOLDS | `resolve_tileset_index` (tiled.rs:277-292) compares the raw gid with firstgid. The 0x80000000/0x40000000/0x20000000 (and 0x10000000 hex-rotate) bits make gid huge, so it resolves to the last tileset with an enormous local id. Flip info is not stored anywhere (`Option<TileId>` cells). |
| 3 | No renderer | STILL HOLDS | No tilemap reference in render/graphics/engine. The demo draws one `Renderable` per tile. |
| 4 | Collision rects never become colliders | PARTLY FIXED | `Tilemap::tile_colliders()` (map.rs:158) plus `TileColliders` now exist. `generate_collision_rects` is still standalone and unused by physics. |
| 5 | Autotile and animation exist but nothing applies them | STILL HOLDS | No call sites for autotile; animation data never read. |
| 6 | Isometric/hex are dead enum variants | STILL HOLDS | map.rs:7-19; `world_to_tile` is orthogonal only (map.rs:74); loader maps "hexagonal" to Flat only (tiled.rs:298). |
| 7 | `visible` defaults to false when absent | NEW | `RawLayer.visible: bool` with `#[serde(default)]` (tiled.rs:209) defaults to false (Tiled always writes it, so only hand-written or minimal JSON is hit). |
| 8 | Base64/zlib/gzip/zstd tile data unsupported | NEW | `data: Vec<u32>` (tiled.rs:213). A `.tmj` saved with non-CSV encoding fails Parse. |
| 9 | Group layers and image layers dropped silently | NEW | tiled.rs match arm `_ =>`. Children of a group layer, and any layer offset (`offsetx/offsety`), are lost. |
| 10 | Tile objects (with `gid`) not parsed; `class` field missing (Tiled 1.9+ writes `class`, loader reads `type`) | NEW | RawObject (tiled.rs:~197-222) has no gid/class/text/template. |
| 11 | `has_tiles` never cleared; `dirty` never consumed | NEW | manager.rs:~118-126. `visible_chunks` iterates a HashMap, so order is arbitrary. |
| 12 | `TileColliders` is a stale snapshot after `set_tile` | NEW | No link between `TilemapManager::set_tile` and any `TileColliders` (manager.rs:97-127). |
| 13 | Out-of-range gid with no matching tileset is silently skipped; `tile_count` is not checked against local ids | NEW | tiled.rs:528-538 (`resolve_tileset_index` returns Some for any gid >= first firstgid). |
| 14 | Tileset `image_path` not resolved or loaded; external tsj image not rebased | NEW | tiled.rs:`build_tileset` copies `raw.image`; no loader reads it. |
| 15 | Loader bypasses asset root and registry | NEW (known in follow-ups.md:87) | `std::fs::read_to_string` at tiled.rs:~452, :~485. |
| 16 | Two collision code paths disagree | NEW | `generate_collision_rects` (solid only, per-layer tileset) vs `tile_colliders` (solid, slope, one-way, rank). |
| 17 | `TileProperties` stored per tile in a `HashMap<TileId, ..>` with `get_properties` cloning the custom map | NEW (perf) | tileset.rs:90; `tile_colliders` and `is_solid_at` call it per cell per layer. |
| 18 | Chunk grid built from map size, not layer size | NEW (minor) | manager.rs:60-82 uses `tilemap.map_width/height`; layers can differ in size (`TilemapLayer::new(w,h)` independent). |

## 3. Who uses it

- Demo `examples/demos/tilemap/main.rs` (371 lines). It builds a map in code with `image_path: String::new()` ("no image, sprite proxies"), registers it in `TilemapManager`, and draws one coloured `Renderable` entity per tile (`TileVis`, rebuilt wholesale on every edit by despawn and respawn, lines 131-190). It shows queries, hover, `set_tile`, and `generate_collision_rects` outlines. It does not use Tiled loading, `tile_colliders`, autotile, or any tileset image.
- Games: none use `Tilemap`. flappy uses `CompositeTile` (unrelated). leap-king and bullet-hell, breakout, nigh_t: no tilemap (grep found no `Tilemap` in examples/games). leap-king uses `ryo2d_procgen::SeededRng` only.
- Demo `character` and `benches/physics.rs` build `TileColliders` by hand from ASCII rows (examples/demos/character/main.rs:166-188; benches/physics.rs:145). They do not use `Tilemap`.
- Demo `procgen` draws `GenerationGrid` with `ui.draw_rect` per cell (examples/demos/procgen/main.rs:242). Demo `pathfinding` is not tile-based (inferred, not opened).
- Skills: `ryo-api` Tilemaps section (.claude/skills/ryo-api/SKILL.md:1066-1078, says "the renderer does not draw tilemaps yet"); `ryo-character` (TileColliders and the `tilemap.tile_colliders()` path, lines 244-258, 665); `ryo-physics:172`; `ryo-game-dev` and `ryo-engine-dev` mention `tilemaps()`/the resource. Copies live in `src/cli/skills/` (embedded). No tilemap-specific skill exists. ryo-procgen shows no tilemap bridge.
- Procgen (`crates/ryo2d-procgen`, 4,610 lines): outputs `GenerationGrid` of `TileType {Empty, Floor, Wall, Door, Water, Custom(u32)}` (config.rs:126-140), with `Region`s and `Connection`s. Its only bridge is `GenerationGrid::to_navigation_grid(cell_size)` (grid.rs:388). It has no conversion to `Tilemap`, `TileColliders`, or `TileCell`, and no reference to tilemap anywhere in `crates/` (grep). Games must hand-map TileType to tiles.
- Docs and tests that mention it: the audit, the tilemap-features backlog, the specs above, and `docs/architecture/overview.md`.

## 4. Integration points a rewrite must respect

- **ECS / resources:** `TilemapManager` is a resource keyed by `Entity`, inserted in engine/mod.rs:409; `ctx.tilemaps()` is public API in the prelude and skills (breaking is acceptable per memory "Breaking changes OK"). `forget_despawned_tilemaps` runs in `Last` and after `SceneCleanup::WORLD`. A component-based design would make that bookkeeping unnecessary (inferred). The demo-kit resets tilemaps only by despawning page entities (examples/demos/AGENTS.md:81 lists "tilemaps" among state that carries over).
- **Render batch and draw order:** the renderer gathers everything from queries each frame with no retained cache (render/AGENTS.md:53). The draw list sorts by `(z, DrawKind{Entity, Text, Sprite, Trail}, sequence)`; a tilemap needs a z per layer, probably an Entity-kind item per layer or chunk. `DrawPayload` has Sprite(index), Triangles(index), Glyph(index) (render/draw_list.rs:~28-38). The batch has two streams: 128-byte instanced quads (`SpriteInstance`, graphics/sprite_gpu.rs:39) and indexed `Vertex2D` triangles (72 bytes). It merges equal `DrawState` runs. Quads outside each camera's `CameraView::bounds` are culled, and sprite UV samples outside `uv_bounds` are transparent (atlas-bleed protection), which matters for tileset margins and spacing. `RenderLayers` (0..=31) per drawable and camera, per-camera `only_camera`, lighting on/off per item, interpolation (`PreviousGlobalTransform`) and `Renderable::anchor` conventions (top-left for grids) all apply. A UI renderer is separate and untouched.
- **Retained rendering precedent:** the UI path has retained fragments and cached runs (render/AGENTS.md:92-104). The world batch has none, so chunk-retained instances would be new (inferred).
- **Physics:** keep `TileColliders` (physics/tiles.rs) as the single collider path: it already does outlines, slopes, one-way, rebuild-by-generation, `Disabled`, kinematic carry, and navigation (`block_tiles`). Its API (`TileCell`, `from_cells`, `set`) is what the physics spec says "the later tilemap rewrite calls". Today `tile_colliders()` is a one-shot copy; a rewrite should either emit/refresh it on edit or generate it automatically. Rebuild cost is whole-map per change (spec lists incremental rebuild as out of scope).
- **Asset registry:** `Tilemap` and `Tileset` as registry assets need an `AssetLoader` registered in `insert_registry`, `.tmj`/`.tsj` extensions (and possibly an own `.ron` format), tileset image `Handle<Image>` as a dependency (so the map is ready only when images load), paths relative to the asset root (asset/path.rs), `check()` hook for validation, `ReloadFailed` keeping the last good value, and `AssetEvent<Tilemap>` for rebuilding chunk caches. Only one follower may use a store's change list (asset/AGENTS.md:18); the renderer holds ids, never handles (asset/AGENTS.md:68). The design spec says tilemaps join "when they are rewritten as a feature module".
- **Procgen:** it depends on `ryo2d` (so a bridge `GenerationGrid -> Tilemap/TileColliders` can live in procgen, matching the "optional crates depend on core, never reverse" rule) or the core can accept a plain `Vec<TileId>`/grid. TileType needs a mapping table to tile ids or terrain.
- **Demo-kit:** the demo page is one `DemoPage` (examples/demos/tilemap/main.rs); rules in examples/demos/AGENTS.md (WASD, entity cleanup by `SceneCleanup::ALL`, "keep feature code plain ryo2d"). A rewrite replaces the demo wholesale; the current one never loads art. Its Cargo/Ryo.toml are tiny.
- **Skills and docs to update:** ryo-api (Tilemaps), ryo-character (tile colliders from a map), ryo-engine-dev, ryo-game-dev; the CLI copies in `src/cli/skills/`; `src/core/tilemap/AGENTS.md`; docs/backlog/tilemap-features.md; docs/backlog/follow-ups.md:87. AGENTS.md rule: log deferred items in docs/backlog/follow-ups.md at merge.
- **Tests that pin today's API:** 98 unit tests inside the module (chunk 8, layer 6, collision 14, tileset 9, tiled 29, manager 12, autotile 9, map 11), 6 in tests/integration/tilemap.rs (338 lines), 24 in physics/tiles.rs, none in tests/e2e (the only "tile" hits in tests/e2e/gpu_world.rs are `CompositeTile` and trail tiling). There is no GPU or end-to-end test of tilemap drawing, no test of a multi-tileset layer, flip bits, or base64 data.

## 5. Keep versus rewrite

Keep (with reasons):
- `physics/tiles.rs` `TileColliders`/`TileCell`/`SlopeCorner`: finished, tested (24 tests), benchmarked in benches/physics.rs, used by character demo and navigation; it is the intended target of the rewrite per the physics spec.
- `compute_bitmask_*` and `AutoTileSet` bit constants: small, tested, correct; need a layer that applies them on set (and a Tiled terrain/Wang import).
- Tileset UV maths (`tile_uv`, spacing/margin): correct and tested; will be needed, although it should be precomputed per tileset (not per call) and guard `columns == 0`.
- `greedy_merge` rectangle merging (collision.rs): sound algorithm; useful for `solid_rects` or nav, though `TileColliders` already has its own merge.
- Chunk coordinate helpers (`chunks_along_axis`, `chunk_tile_range`, `chunk_visible`) and 16x16 choice: small, fine as a starting point for a retained chunk renderer.
- `TiledObject`/`TiledShape`/`TiledValue` parsing and the loader's test fixtures (object shapes, properties, parallax): reusable once extended (class, gid, text, layer name).
- The "dense Vec, entities only for chunks and behavioural tiles" design decision in AGENTS.md: sound; keeps memory flat and lookup O(1).

Rewrite:
- `TilemapManager` (Entity-keyed resource with a HashMap copy of the map): replace by components on the tilemap entity (map/layer data) so queries, `Disabled`, hierarchy, transforms, `RenderLayers` and cleanup work without bookkeeping; its dirty/has_tiles logic is incomplete.
- `TilemapLayer` cell type: `Option<TileId>` cannot carry flip/rotation, per-cell tileset, or animation state; one `tileset_index` per layer is the cause of bug #1. A cell needs (tileset, local id, flip bits) or a global id with flags.
- `load_tiled_map`: move to an `AssetLoader` on the registry (asset root, image dependencies, hot reload, web), fix bugs #1, #2, #7-#10, #13, #14, add TMX or document JSON-only, base64/zlib data, group layers, layer offsets, tile collision shapes, tile objects, class.
- Rendering: new; chunked retained quad (or mesh) batches per layer with culling, flip bits, tileset atlas-bleed handling, animation on a shared clock, opacity, parallax, per-camera layers and lighting, drawn in z order against sprites; per-tile entities are not viable beyond small maps.
- Collision glue: one path from tilemap to `TileColliders` that stays in sync on edits (and drops or folds `generate_collision_rects`'s duplicate semantics).
- Orientation: either implement isometric/hex coordinates or delete the dead enum variants.
- Demo, skills and procgen bridge: new demo with real art; skills tilemap section; procgen `to_tilemap` helper.
