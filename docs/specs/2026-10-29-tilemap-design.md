# Tilemap (6a) Design

## Overview

Ryo2D's tilemap support today is a data model and a Tiled JSON loader with no renderer: the only demo spawns an entity per tile, no game uses it, the loader reads files from the working directory outside the asset registry, layers that mix tilesets take the last tile's tileset, and flipped tiles resolve to the wrong ids (`docs/research/2026-10-29-tilemap-recon.md`). How Godot, Unity, Tiled, LDtk and the Bevy crates do it is surveyed in `docs/research/2026-10-29-tilemap-survey.md`. This sub-project replaces it with a professional tilemap: levels made in the editors indie studios use, drawn fast at any map size, colliding correctly, carrying the designer's objects and data into the game, and reloading live while the game runs.

It is the first of two. 6a covers orthogonal maps. 6b, straight after, adds isometric and hexagonal maps and Tiled's infinite maps; 6a's data model leaves room for them.

The owner's bar, as for 5b: nothing someone could point to and say is not professional.

## Goals

1. **Levels made in Tiled and LDtk.** The engine reads both editors' files directly:
   - Tiled: maps (`.tmx`, `.tmj`), external tilesets (`.tsx`, `.tsj`), object templates (`.tx`, `.tj`), and world files (`.world`).
   - LDtk: projects (`.ldtk`) and external level files (`.ldtkl`).
   - Tile layer data in every encoding the editors write (CSV, base64, and gzip, zlib or zstd compressed).
2. **Maps are registry assets, with hot reload.** A map, a tileset and an LDtk project load by asset path like an image, and depend on their tileset images. Saving in the editor updates the level in the running game; the camera and the player keep their state. Maps work on the web from the files the build embeds, as every other asset does.
3. **Correct tile data.** Every tile keeps its flips and rotations (Tiled's flip bits, LDtk's flip flags). A layer may use several tilesets. Per-tile custom data (Tiled properties and classes, LDtk enum tags) is readable at runtime, typed.
4. **Fast drawing at any map size.** Tiles draw in cached chunks through the world batch, so lighting, cameras, `RenderLayers`, z-order and post-processing apply to them as to sprites. A frame's cost follows the tiles on screen, not the map's size. Layers keep their editor opacity, tint, offset and parallax. Animated tiles animate on game time (they pause and slow with the game) without rebuilding anything. A layer can sort with characters by y, for top-down scenery a character walks behind.
5. **Collision from the editor.** Collision shapes drawn per tile in Tiled's collision editor, or LDtk's IntGrid values, become physics colliders: full tiles merge into large outlines as `TileColliders` does today; one-way tiles, slopes and other authored shapes keep their shape. Colliders stay in sync with runtime edits, and an edit re-collides only the chunk it touched.
6. **The designer's objects arrive as entities.** Tiled objects (including tile objects and templates) and LDtk entities spawn as entities carrying their name, class and typed custom fields, positioned in the world, so a game reacts to `"player_start"` or `"coin"` by class. A tile can be marked to spawn as an entity instead of being drawn (a breakable brick, a spike with behaviour).
7. **Maps built and changed in code.** A game builds a map in code or edits a loaded one (set a tile, fill a region, clear), for procedural levels and destructible terrain; every change redraws and re-collides only what it touched. Runtime auto-tiling picks edge and corner tiles from the Wang sets a designer defined in Tiled, so code-built maps use the designer's terrains. The procgen crate turns its grids into maps.
8. **Worlds of many levels.** An LDtk world and a Tiled world file load as a set of levels with their positions and neighbours, so a game can move between rooms and load the ones near the player.
9. **Proof.** A tilemap demo with real CC0 art (Kenney's tilesets) shows each goal, including a big-map speed page with the targets below measured; breakout's ten levels become Tiled maps; the character demo's hand-built level becomes an LDtk level. A `ryo-tilemap` skill teaches all of it.

## Non-goals (6a)

- Isometric and hexagonal maps, and Tiled's infinite maps: 6b. A map with another orientation, or an infinite map, fails to load with an error naming 6b's feature, rather than loading wrong.
- A tile editor inside the engine. Tiled and LDtk are the editors.
- Running LDtk's auto-layer rules at runtime. LDtk resolves them in the editor and saves the tiles; code edits of an LDtk level place tiles directly or use Wang sets.
- Running Tiled's automapping rules at runtime (they run in the editor).
- Streaming a world's levels automatically. 6a gives positions, neighbours and a helper to spawn the levels near a point; when to load and unload is the game's call.

## Anti-goals (the professional bar)

- A map that loads but draws, collides or carries data differently from what the editor shows.
- A dead option: an enum variant or setting the engine accepts but does not implement (today's isometric and hexagonal variants go; 6b brings them back working).
- A frame cost that grows with the map's size rather than what is on screen, or an edit that rebuilds the whole map.
- Seams or bleeding between tiles at any zoom, filter or camera position.
- A reload that loses the game's state on objects that did not change, or duplicates them.
- Loading from the working directory, or anything that cannot work on the web.

## Decisions

### The owner's decisions (2026-10-29)

The owner agreed to the picture proposed after the recon: both editors, registry assets with hot reload, maps built and edited in code with the procgen crate, objects as entities, chunked drawing through the world batch, collision from per-tile shapes and the IntGrid with one-way tiles and slopes, correct flips with several tilesets per layer and per-tile data, CC0 art in the demo, a big-map speed page, and a sample game's levels moved to Tiled. Isometric and hexagonal maps and infinite maps are to follow as their own sub-project, not be dropped.

After the design's review the owner decided:
- Tiled and LDtk are installed on the development Mac (free desktop apps, not engine dependencies). The builder scripts the demo's and tests' files in the editors' formats and checks them with Tiled's command line; at hand-play the owner opens and saves each in its editor, so anything an editor would rewrite shows up before the merge, and LDtk fills in its auto-tiles.
- Breakout's Cyrillic-typed cells become the bricks they were meant to be (Acceptance).

### My calls (the owner may revisit them in review)

- **Both Tiled formats, read in-house.** TMX is Tiled's default, so reading only JSON would turn most Tiled users away. The readers are the engine's own (the owner keeps the engine in-house), built on libraries already in its build: `serde_json` (TMJ, TSJ, LDtk), `roxmltree` (TMX, TSX, already compiled in for SVG), and `flate2`, `zstd` and `base64` for tile data. No new dependency, and no `tiled` crate. Tiled and LDtk are desktop editors the designer uses; the engine and games never depend on them.
- **Chunks of cached GPU instances.** Each layer's tiles are kept in chunks whose instances are built once and uploaded once, drawn as items of the world batch with the layer's z, culled per camera, and rebuilt only when a tile in them changes. This is the conventional design (Godot's quadrants, Unity's chunk mode, bevy_ecs_tilemap) and keeps tiles inside the existing frame (lighting, cameras, layers, interpolation of the map's transform). A whole layer drawn by a shader from a tile-index texture was the alternative; it fights per-layer lighting, y-sorting and per-camera layers. The chunk size is the builder's, measured.
- **Y-sorted layers by row.** A layer marked to sort with characters draws each row of tiles as an item at that row's y, so a character sorts against it; its cost follows its visible rows.
- **Tilesets padded against bleeding.** A tileset image is drawn from a copy with each tile's edge texels extruded, made as it loads, so no filter, mip level or subpixel camera position samples a neighbour.
- **Reload keeps objects by id.** On a reload, tile layers are replaced; objects are matched by their editor id (Tiled's object id, LDtk's iid): an unchanged object keeps its entity and every component the game added, a changed one is updated in place, a new one spawns, a removed one is despawned. The file wins over runtime tile edits made since it loaded (a reload is a dev-time action).
- **Collision shapes beyond full tiles.** `TileColliders` keeps its merged outlines for full tiles and its one-way and slope cells, and gains authored shapes per tile (rectangles and polygons, any angle, one-way by a property).
- **Edits rebuild the outlines they touch, whole.** Outlines are never cut at chunk borders: a long floor stays one seamless outline, as the character controller and dynamic bodies need. An edit rebuilds the outline loops that pass through the cells it changed (and the authored shapes in them), not the whole grid. The edit target holds for loops spanning up to four chunks; a map that is one vast connected region pays in proportion to that loop's length.
- **Tiled's project file is an asset.** Tiled keeps class and enum definitions, and class members' defaults, in the `.tiled-project` file, and a normal save writes only the members a designer changed; reading maps alone would lose the defaults. The engine reads the project file (the one at the asset root, or the path `Ryo.toml` names) as an asset that maps and tilesets depend on, hot reloaded like them. Fields are typed values (bool, int, float, string, colour, file, object reference, enum, class, list), read by name; no Rust declaration is needed. LDtk files carry every field with its type, so nothing extra is read for them.
- **Tiles block light.** A tile layer can occlude 2D light (on by default for collision layers): its occluders come from the tiles' collision shapes, or from shapes a tileset marks as occluders, merged and rebuilt per edit like colliders, through the existing `LightOccluder2D`. The shadow rewrite, a later sub-project, improves how they look.
- **Tiled 1.12 blend modes.** Normal, add, multiply and screen layers draw with the batch's blend modes. A map using any other blend mode fails to load with an error naming the mode, rather than drawing differently from the editor.
- **LDtk collision by IntGrid name.** An IntGrid value whose identifier is `solid`, `one_way` or `slope_*` collides as that by default; a game can map other values. Tiled collision comes from the tileset's collision shapes, one-way by a `one_way` property.
- **Tiles as entities by class.** A tileset tile whose class a game registers spawns an entity at each place it is used (with its tile, position and data) and is neither drawn nor collided by the map.
- **Breakout's levels as tile layers whose bricks spawn as the game's brick entities**, so physics and feel stay exactly as tuned; the character demo's level as an LDtk level whose IntGrid collides and whose auto-tiles draw.

## Design

### Assets

- `Tileset` (Tiled `.tsx`/`.tsj`, and Tiled's inline tilesets; LDtk tileset definitions): its image as a `Handle<Image>` dependency (paths resolved against the tileset's file, then the asset root), tile size, spacing and margin, and per tile: class, typed properties, collision shapes, animation frames, Wang set membership. Image-collection tilesets (one image per tile) are supported.
- `TileMap` (Tiled `.tmx`/`.tmj`; also made in code): size, tile size, layers in document order (tile layers, object layers, image layers, and group layers with their children flattened with the group's offset, opacity, tint and parallax applied), map and layer properties, and the tilesets it uses as handles.
- `LdtkProject` (`.ldtk`, with `.ldtkl` external levels as dependencies): definitions, worlds and their levels. A level spawns as a `TileMap` does. LDtk's IntGrid, auto-tile, tile and entity layers map onto tile, collision and object data.
- `TileWorld` (Tiled `.world`; an LDtk world): levels or maps with their world positions and neighbours.
- Loading runs on the registry's workers; a tileset image too small for its declared tiles, a gid outside every tileset, an unknown encoding or orientation, or a broken file fails the load with the file and the place in it. A reload that fails keeps the last good value (`ReloadFailed`), as every asset does.

### At runtime

- A map is spawned as an entity holding a handle to its asset (a `TileMap`, or an LDtk level), with a `Transform` like any entity: moving, scaling or parenting it moves the map. Its layers are child entities; each layer holds its tiles in chunks. Despawning the map despawns its layers and the entities it spawned.
- Tiles are addressed by layer and cell. Reading: the tile, its flips, its tileset, its data. Writing: set, clear, fill a rectangle, apply a pattern; world-to-cell and cell-to-world conversions follow the map's transform.
- Objects spawn as child entities with a `MapObject` component (editor id, name, class, shape, typed fields; references to other objects resolved to their entities) and a `Transform` at the object's place; tile objects draw their tile. Games react with an observer or a system on the class; the details of the hook are the builder's.
- Runtime auto-tiling: a layer edit through a Wang set (corner, edge or mixed) updates the edited cells and their neighbours from the tileset's Wang data.
- The procgen crate gains a conversion from its `GenerationGrid` to a `TileMap`, mapping its tile types to tiles or Wang colours.

### Drawing

- Chunks build their instances from the tiles (UVs in the padded tileset, flips, tint, opacity) when first drawn and when a tile in them changes; a frame only picks the visible chunks of each camera. Animated tiles carry their frames' description in the instance and resolve the frame from game time in the shader.
- Layers draw at their z among entities; the editor's layer order gives consecutive z values above the map's base z, which the game can set. Per-layer lit or unlit, `RenderLayers` from the map, parallax per camera as `ParallaxLayer` does, and the map's `Disabled` and visibility all apply.
- Image layers draw as sprites at their offset, repeating if the editor says so.

### Collision

- A map builds its `TileColliders` from tiles' collision shapes (Tiled) or IntGrid values (LDtk), across all layers that collide (a layer property turns a layer's collision off). Edits update the affected cells and rebuild that chunk's colliders before the next physics step. Collision layers, masks, friction and restitution come from map or layer properties, with defaults.
- Navigation keeps reading `TileColliders` (`NavigationGrid::block_tiles`).
- Occluding layers keep `LightOccluder2D` entities in step with their tiles, rebuilt per edit like colliders.

### Hot reload

- A saved map, tileset, template, LDtk project or level, or tileset image updates every map spawned from it at the start of the next frame: tile layers and colliders rebuild, objects reconcile by id (above). A tileset image reload redraws without rebuilding colliders.

### The web

- Maps load from the embedded `MemorySource` like every asset; nothing reads the file system directly.

### Headless and tests

- Loader tests read fixture files as Tiled 1.12 and LDtk 1.5 write them, saved by the editors themselves where they can be (both are installed; scripted files follow the documented formats exactly and are opened and saved in the editors at hand-play).
- GPU tests pin drawing: flips and rotations, a layer mixing tilesets, an animated tile's frame at a given game time, opacity and tint, parallax, culling at chunk edges, and no seams under zoom and subpixel cameras.
- Collision tests pin authored shapes, one-way tiles, slopes, merging, and an edit rebuilding only its chunk.
- Reload tests pin object reconciliation and a failed reload keeping the last good map.

## Public API and migration

- The old module goes: `Tilemap`, `TilemapLayer`, `TilemapManager` and `ctx.tilemaps()`, `load_tiled_map`, `TiledObject`, `generate_collision_rects`, and the orientation enum's dead variants. The bitmask auto-tile helpers, the tileset UV maths, chunk coordinate helpers and greedy rectangle merge are kept where they serve the new design. Breaking changes are fine (`breaking-changes-ok`).
- The prelude exports what a game needs to load, spawn, read, edit and react to maps; the builder settles the names, shaped like the registry's and the audio module's (handles, components, a context accessor where an ECS query is not enough).
- The skills that mention tilemaps (`ryo-api`, `ryo-character`, `ryo-physics`, `ryo-game-dev`, `ryo-engine-dev`, `ryo-procgen`, `ryo-assets`) are updated, and a `ryo-tilemap` skill is added; their Rust examples compile (`tests/skill-examples`).

## Acceptance

### Correctness

- The demo's pages each show one goal working with real art: a Tiled map and an LDtk level side by side, flips and mixed tilesets, animated tiles pausing with the game, a y-sorted layer, collision with one-way tiles and slopes for a character, objects spawning, runtime edits with Wang auto-tiling, a procgen dungeon as a map, a world of rooms, and hot reload (save a map while it runs).
- Breakout plays its ten levels from Tiled maps exactly as before (scripted hand-play compares frames against the old levels). The owner's call: the seven cells of levels 5 and 7 typed with Cyrillic letters (Р, О), which the old loader skipped as gaps, become the red and orange bricks they were meant to be; those two levels are left out of the frame comparison.
- The character demo plays its level from LDtk with the same feel.
- Every bug in the recon's table (`docs/research/2026-10-29-tilemap-recon.md`, section 2) that the new design could still meet has a test that would have caught it.

### Targets (my calls; measured, guarded like 5a's and 5b's)

- A static 1024x1024 map of four layers adds at most 50 µs of CPU a frame over an empty frame, with a 1080p view of 16 px tiles, whatever the map's size.
- Editing one tile redraws only its chunk and rebuilds only the outline loops through it (and its occluders), each within 100 µs for loops spanning up to four chunks.
- A 256x256 four-layer Tiled map in compressed base64 loads off the main thread within 20 ms, and adds no hitch to the frame it lands in.

## Documentation

- `src/core/tilemap/AGENTS.md` rewritten for the new module; `src/core/render/AGENTS.md` and `src/core/physics/AGENTS.md` for chunks and authored collision shapes; `src/core/asset/AGENTS.md` for the new asset types.
- `docs/backlog/tilemap-features.md` folded into 6b's scope and what stays later.

## Risks

- **Fixture fidelity.** Editors write details the documentation underplays; fixtures saved by Tiled and LDtk themselves remove the guesswork.
- **LDtk's pace.** LDtk 1.5.3 (January 2024) is still current; the loader follows its documented JSON schema version and fails clearly on a newer one it cannot read.
- **Per-chunk collider rebuilds** change `TileColliders`' all-or-nothing rebuild, which the character controller's floor detection leans on; its tests guard the change.
- **Retained chunks in the world batch** are new: the batch has no retained items today (the UI renderer does).
- **Partial texture uploads on Apple GPUs.** The glyph atlas met a GPU fault with uploads over part of a 16x16 texel block (2026-10-29, `glyph_atlas::BLOCK`); the padded tileset textures are written whole, and any partial texture write follows the same whole-block rule.

## Out of scope

- Isometric, hexagonal and infinite maps (6b).
- Tiled's text objects beyond their placement, and its oblique orientation (1.12).
- Per-tile runtime overrides of tileset data (Godot's runtime tile data).
- Saving maps back to the editors' formats.
