# Tilemaps (Sub-project 6a): speed

Spec: `docs/specs/2026-10-29-tilemap-design.md` (Acceptance, Targets). The 6b feature list is `docs/backlog/tilemap-features.md`.

## 6a: measured

Measured on 2026-10-30 on an Apple M5 Pro (18 cores, Metal, macOS 27.0.1), a shared machine: other work (three `BanjoRecompiled` processes and a Python job at about 16% of a core each) ran through the evening and pushed the 1-minute load between 4 and 16. Medians move with the load, so every run's 1-minute load is given.

**Method.**
- `benches/tilemap.rs` (its module docs say what each guard does) ran three times on the branch, in the workspace's own `target/` (the bench profile: release with debug info). Each run waited until the 1-minute load was 4 or less; the loads at the runs' starts were 4.00, 3.74 and 3.56, rising to 4.6 to 5.5 by their ends as the other work resumed.
- Engines run at 1920 x 1080 with 16 px tiles at zoom 1, headless (`HeadlessEngine`) on the GPU. A frame's cost is the wall time of the whole step, with the GPU finished before each step outside the measure (as `benches/ui_render.rs` does); the static guard's engines step in turn, one frame each. The landing frames are paced at 60 Hz with a spin wait.
- Each verdict holds the median (the landing, its slowest frame over the run's median ordinary frame, with a run over the limit measured once more and the better run held); the 95th percentile and the maximum are printed beside it.
- The numbers before a fix come from the guard's own runs while the fixes were made, at the loads noted (the machine was rarely under 4 then).
- The one-off timings (ignored tests) ran once at a load of 3.79, in release.

### The targets

| Target | Measured, three runs: median (95th percentile, max) | Verdict |
|---|---|---|
| A static 1024 x 1024 four-layer map adds at most 50 µs of CPU a frame over an empty frame (`static_frame`, the map colliding and casting shadows, the camera flying) | +100.0 (238.2, 578.6), +104.0 (249.0, 2,114.0), +102.3 (253.9, 594.7) µs | **missed**, reported: transform propagation (below, Limits) |
| An edit's chunk rebuild within 100 µs: the skyline | 36.8 (159.3, 399.2), 38.5 (124.5, 238.3), 42.1 (136.6, 275.1) µs | **met** |
| An edit's chunk rebuild: the serpentine | 35.0 (126.0, 395.8), 40.0 (146.8, 1,077.1), 37.5 (128.3, 1,178.3) µs | **met** |
| An edit's loop rebuild within 100 µs: the skyline (a loop of 1,332 edges over four chunks) | 69.2 (98.5, 250.9), 67.9 (90.0, 148.4), 70.5 (104.7, 196.2) µs | **met** (198.7 µs before this task's fix, load 3.6) |
| An edit's loop rebuild: the serpentine (one cell wide, 4,128 edges, filling four chunks) | 64.5 (98.3, 185.3), 65.2 (87.5, 186.0), 66.4 (99.9, 191.7) µs | **met** (547.0 µs before, load 3.6) |
| An edit's occluder rebuild within 100 µs: the skyline | 51.7 (71.5, 130.6), 51.1 (74.7, 130.5), 52.3 (81.0, 149.3) µs | **met** |
| An edit's occluder rebuild: the serpentine | 86.6 (124.3, 264.7), 88.5 (135.5, 261.9), 88.0 (128.6, 363.3) µs | **met** (121.5 µs before, load 3.6) |
| A 256 x 256 four-layer TMX in base64 and zlib loads off the main thread within 20 ms (with its TSX and PNG, fresh files) | 1,354.0 (1,512.8, 1,512.8), 1,352.7 (1,560.0, 1,560.0), 1,368.7 (1,551.2, 1,551.2) µs | **met** |
| It adds no hitch to the frame it lands in: the slowest landing frame within 0.5 ms of an ordinary frame | the better run's slowest +3,353.5, +3,250.7, +3,259.2 µs: the first landing, which pads the tileset (the others: medians +562 to +621 µs) | **missed**, reported: the first draw (below, Limits) |
| A frame drawing a static map allocates nothing (`tests/tilemap_alloc.rs`) | still and panning, the map's frame makes as many allocations of the engine's own as an empty frame (229 and 229, dev profile); a padded tileset cache hit makes none | **met** (the map's frame made 19 more before this task's fixes) |
| Steady frames create no bind groups, with an image layer now among the map's layers (`steady_frames_create_no_bind_groups`) | none in three steady frames | **met** |

### The static frame (`static_frame`)

The map: 1024 x 1024 cells of 16 px in four layers over one 256 x 256 atlas: a background on every cell, cave rock from value noise (a full-cell collision shape, so it collides and casts shadows), props on about one open cell in 16 and front details on one in 64. Built in code, landed, then 120 frames to warm up; 2,000 frames each of every engine, interleaved; the camera flies a long loop over the map at about 700 px/s, so new chunks come into view every few dozen frames; the frames that build them cost more, and the 95th percentiles and maxima include them.

| Each engine, its step over the empty frame's median | Run 1 (4.00) | Run 2 (3.74) | Run 3 (3.56) | Its render step over the empty frame's |
|---|---|---|---|---|
| An empty frame: the step, and its render step | 146.6 µs (103.1) | 156.8 µs (108.4) | 159.8 µs (110.4) | |
| The map, colliding and casting shadows (held): 4,096 entities, 2,881 of them children, 2,378 occluder chains | +100.0 µs | +104.0 µs | +102.3 µs | +43.7, +47.3, +46.8 µs |
| The map drawn only (no collision, no shadows): 4 children | +47.2 µs | +50.8 µs | +48.4 µs | +45.5, +48.1, +46.6 µs |
| Colliding, without shadows: 503 children (collider parts) | +64.0 µs | +64.9 µs | +67.4 µs | +48.8, +50.3, +52.3 µs |
| Casting shadows, without collision: 2,382 children (occluder chains) | +90.3 µs | +91.0 µs | +89.6 µs | +44.9, +46.6, +45.9 µs |
| Its props layer y-sorted | +187.1 µs | +192.0 µs | +191.7 µs | +129.5, +133.1, +133.5 µs |
| Lit by one shadowed light following the camera, over an empty lit frame (206.1, 221.8, 224.3 µs) | +149.7 µs | +145.5 µs | +146.2 µs | +148.6, +151.7, +151.6 µs |

- **Drawing the map costs about 48 µs**, nearly all in the render step (+46 µs): about 16 to 20 visible chunks a layer, one draw each per tileset, each bound and encoded on its own.
- **The miss is the map's child entities.** Every collider part and occluder chain is a child entity, and they cost about 17 ns each a frame: 503 parts add about 16 µs, 2,378 chains about 42 µs, so the map with both reads about 102 µs. A profile of the map frame (`--profile-time 20 '^static_frame$'`) charges 22% of it to `propagate_transforms`: once anything moved (here the camera, in a game a player), each of its three runs a frame (physics' own schedule, the fixed step's end and `PostUpdate`) walks every entity of every hierarchy, recomputing only what changed but visiting all. The fix is propagation's, outside the tilemap: see Limits.
- **A y-sorted layer** draws each visible row of each chunk as an item of its own (one per row, chunk, tileset and camera), and costs about another 90 µs here; no target.
- **Lit frames** pay for every occluder in the map: about 46 µs more than the unlit map over the same light (see Limits).

### The edit (`edit`)

The map: 1024 x 1024 cells in four layers, the ground holding 3 x 3 islands every 8 cells (about 16,000, packed into about 1,000 chunk colliders, with an occluder chain or more each) and two loops each in its own row of 4 x 1 chunks: a jagged skyline (128 columns up to 24 cells high) and a one-cell serpentine filling the four chunks. A tile on the skyline's top (64, 20) and a bump on the serpentine (64, 64) are set and cleared 400 times each, an ordinary frame between edits, as a game edits.

| Part, median (run 1, 2, 3) | Skyline | Serpentine |
|---|---|---|
| Collision sync (the chunk classified again across four layers) | 21.8, 21.7, 22.0 µs | 20.1, 20.3, 20.3 µs |
| Tile collider rebuild and collider sync (retrace, reshape, the Rapier shape) | 47.1, 45.9, 48.0 µs | 43.9, 44.4, 45.6 µs |
| Loop rebuild (the two above, each edit; held) | 69.2, 67.9, 70.5 µs | 64.5, 65.2, 66.4 µs |
| Occluder rebuild (the chunk's chains, commands applied; held) | 51.7, 51.1, 52.3 µs | 86.6, 88.5, 88.0 µs |
| Chunk rebuild (the render step after the edit over an ordinary one; held) | 36.8, 38.5, 42.1 µs | 35.0, 40.0, 37.5 µs |
| Back to back, no frame between (data in cache): loop rebuild; occluder rebuild | 51.7, 51.4, 52.4; 40.6, 40.6, 41.1 µs | 49.5, 49.8, 49.5; 70.0, 69.8, 69.4 µs |
| The edit frame's whole step over an ordinary frame's (no target) | +311.7, +300.6, +310.3 µs | +309.8, +305.5, +307.9 µs |

- **A frame between edits matters.** The first runs read the loop rebuild at 198.7 µs (skyline) and 547.0 µs (serpentine) with a frame between edits, and 53 and 118 µs back to back: the retrace walked every edge of the loop through the outlines' map of points (about 200,000 points on this map), and once a frame has passed each step is about a cache miss. The physics module's own timing (`one_tile_edit_timing`, edits back to back) never saw it. Fixed: see the fixes.
- **The edit frame's whole step** pays for the edit's new occluder entities and reshaped collider through transform propagation: the edit changes entities, so its runs walk the map's 17,000 children (34% of the `edit` bench's profile). Not part of the targets; see Limits.

### The load and the landing (`load`)

Twenty loads of fresh files each (map, tileset and image under new names, the asset workers started by an untimed load first), pumping on the main thread with a yield between pumps:

| | Run 1 | Run 2 | Run 3 |
|---|---|---|---|
| Load: TMX, TSX and PNG, `load` to ready with its dependencies (held) | 1,354.0 µs (1,512.8, 1,512.8) | 1,352.7 µs (1,560.0, 1,560.0) | 1,368.7 µs (1,551.2, 1,551.2) |
| The main thread's pumps meanwhile | 625.6 µs | 625.5 µs | 584.1 µs |
| A level change (the tileset loaded already) | 1,297.8 µs | 1,300.9 µs | 1,281.8 µs |

The landing: an engine at 60 Hz loads and spawns a fresh copy of the map every 40 frames (its tileset held loaded) and despawns it 32 frames later, twenty times a run; each landed one frame after its spawn but one.

| Over the run's median ordinary frame (about 384 to 437 µs) | Run 1, its two measurements | Run 2 | Run 3 |
|---|---|---|---|
| The asset landing: median, max | 0.0, 2.7; 0.0, 47.5 µs | 0.0, 18.2; 0.0, 59.6 µs | 0.0, 23.0; 0.0, 138.4 µs |
| The first map landing (pads the tileset, makes its texture) | +4,985.4; +3,353.5 µs | +4,131.0; +3,250.7 µs | +3,569.6; +3,259.2 µs |
| The other map landings: median (95th percentile, max) | +609.6 (807.9); +561.8 (631.7) µs | +574.0 (692.6); +567.9 (879.0) µs | +579.9 (719.8); +621.0 (884.8) µs |
| Held: the slowest landing frame, the better measurement | +3,353.5 µs | +3,250.7 µs | +3,259.2 µs |

- **The first landing** pads the tileset (each tile copied into its own cell, edges extruded) and makes its texture, whose mip chain and border are worked out on the CPU in that frame (`atlas::mip_chain`, `write_mips`, `extrude_border`: about a quarter of the `landing` bench's profile before the fix below).
- **Every later landing** draws the map for the first time: about 27,000 tiles in view at 1080p, each a 128-byte instance built, copied into the chunk arena and uploaded (about 3.5 MB), with the landing's entities, colliders and chains spawned. About +0.6 ms.
- Before the fix that keeps paddings, every landing padded again (the map landing's median +2,532.8 and +2,701.9 µs at load 10).

### The fixes made in this task

| Fix | Before | After |
|---|---|---|
| **Outline splice** (`physics/tiles/outline.rs`): where an edit touched one loop and it stays one, the retrace keeps its id and copies its old corners up to the points whose edges changed, looking up only those; splits and joins fall back to the whole trace | loop rebuild with a frame between edits: skyline 198.7 µs, serpentine 547.0 µs (load 3.6); back to back in `one_tile_edit_timing`: 30 µs (retrace 23) and 88 µs (71) | 67.9 to 70.5 µs and 64.5 to 66.4 µs (with the collider sync now counted); back to back 8.9 µs (retrace 2.3) and 22.5 µs (6.6) |
| **Occluder chains** (`tilemap/occluders.rs`, `chains`): each point's edges a run of the edges sorted by start, found through one map made at its full size; no list a point, no map grown from empty, no chain made for every start tried. The same chains (checked against the old function on 20,000 random edge sets) | serpentine occluder rebuild 121.5 µs (load 3.6); in it `chains` took 48.5 µs of a dense chunk's 992 edges | 86.6 to 88.5 µs |
| **Paddings kept** (`tilemap/padded.rs`): a padded tileset is kept for 120 frames after a map last drew it (as an undrawn chunk is), so a level respawned or changed with the same tileset draws with it | each landing padded again: the map landing's median +2.5 to +2.7 ms (load 10) | the landings after the first: medians +562 to +621 µs |
| **No allocation of the engine's own for a static map's frame**: transform propagation keeps its walk's stack between runs (`ecs/propagate.rs`), the draw list sorts unstably (`render/draw_list.rs`: each item's sequence is its own, so the order is the same), and a chunk of one tileset skips its stable sort (`render/tiles.rs`) | the map's frame made 248 allocations of the engine's own against an empty frame's 229: 18 for propagation's stack, 1 for the sort | 229 against 229 |

### The physics bench (Task 19's floor tie)

`cargo bench --bench physics -- characters` ("100 characters on tiles"), three alternated rounds of the build before the floor tie (877700c5^'s `character.rs` swapped in place) and after (HEAD), each run started at a load of 4 or less (3.63 to 3.99): before 639.6, 643.6 and 562.5 µs, after 670.5, 585.5 and 631.4 µs; medians of the rounds 639.6 and 631.4 µs, -1.3%. The rounds swing by ±60 µs either side. The guard (no slower than before by more than 5%) is **met**.

### The carries from earlier tasks

- **Static frame (Task 17's demo, 60 to 67 µs of render CPU in the dev profile at 960 x 600, the camera still):** in release at 1920 x 1080 with the camera flying, the map drawn only adds 47.2 to 50.8 µs (its render step +45.5 to +48.1 µs); with collision and shadows 100.0 to 104.0 µs (above).
- **Per-frame padded-cache lookups:** a hit costs 0.04 µs for an atlas tileset and 8.0 to 8.25 µs for a 1,000-tile collection (its stamp reads every tile's image version), once a tileset a frame.
- **One draw per (chunk, tileset, camera), and per row on a y-sorted layer:** a y-sorted props layer costs the frame about 90 µs more (above).
- **Steady unlit frames +0.05 to +0.08 ms on a dense 256² map with 1,024 parts and chains (Task 11):** on the 1024² cave map, 503 collider parts add about 16 µs and 2,378 chains about 42 µs to a frame whose camera moves (above).
- **Collision edit (Task 10: 37 µs skyline, 87 µs serpentine; the one-chunk reclassify about 18 µs more):** measured in the engine with a frame between edits: loop rebuilds of 68 to 70 µs and 64 to 66 µs, of which the collision sync is 22 and 20 µs (above). `one_tile_edit_sync_timing` (back to back, a 1024² four-layer map) reads the sync at 19.0 µs (max 69.8).
- **Occluder edit (Task 11: 45 to 46 µs median, 192 to 275 µs max):** the skyline 51 to 52 µs (max 130 to 149), the serpentine's dense chunk 87 to 89 µs (max 262 to 363). `one_tile_edit_sync_timing` reads 43.5 µs (max 245.6).
- **The IntGrid table rebuilt on every edit step:** not measured on its own: the edit map has no IntGrid layer, and the table holds an entry per IntGrid value name and override (a few), rebuilt with a small allocation on the steps whose `TileLayerData` changed.
- **The 1024² four-layer first build (Task 11: about 75 ms in the step and frame after spawn):** spawn to landed 75.9 to 83.8 ms with frames stepped back to back (the landing job on its worker), the landing frame 5.4 to 5.9 ms, the frame after 0.3 ms. See Limits.
- **The web's inline landing:** a native bench cannot run it; the landing job of the 256² map, spawn to landed with frames stepped back to back, takes 5.0 to 5.1 ms, which the web pays in the frame it is sent. See Limits.
- **Idle step (Task 10: 8 µs at 16,300 parts):** `one_tile_edit_timing`'s fixed step with physics and no edit reads 3.9 to 4.1 µs on that grid (1,021 parts); `one_tile_edit_sync_timing`'s idle fixed step of a 1024² four-layer map 23.7 µs, its idle collision sync 0.13 µs and occluder sync 0.08 µs.
- **Lit frames gather every map occluder (`engine/extract.rs`), and each shadowed light loops over all of them:** one shadowed light over the 1024² cave map (2,378 chains) costs 145.5 to 149.7 µs over an empty lit frame, against the unlit map's 100 to 104 µs. See Limits.
- **Wang `best_tile`:** 16.4 µs a call at 255 tiles with every flip (`best_tile_timing_on_a_255_tile_set`), as Task 12 measured.
- **Hot reload:** `reload_timing` (a 1024² four-layer map): applying a one-tile save 1.26 to 2.47 ms (the first, cold, the slowest), then 47 to 89 µs for the next step and `PostUpdate`; a save of every chunk 1.78 ms, then 87.6 ms. About 1.3 to 2.5 ms per 4 M cells, against Task 13's 1 ms at a quieter moment.
- **The character controller's floor tie (Task 19):** above.

## Limits

- **Transform propagation walks static subtrees** (the static frame miss). With anything moving, each of the frame's three propagation runs visits every entity of every hierarchy, and a big map's collider parts and occluder chains are thousands of children: about 17 ns each a frame (+53 µs for the 1024² cave map's 2,881), and an edit, which changes some of them, makes the same walk (+300 µs on the edit map's 17,000). The fix belongs to `ecs/propagate.rs`, a design change outside the tilemap: walk only the subtrees that hold a change, by marking each changed entity's ancestors (as bevy's `TransformTreeChanged` does) or by walking up from the `Placement` query's changed entities to their roots. The map would then cost about what drawing it does (+48 µs) plus about 7 µs of physics for its parts: right at the target, so the draw path needs a margin too (fewer draws: neighbouring chunks of a layer whose instances are contiguous could share a draw).
- **The landing frame draws the map for the first time** (the landing miss). The first landing of a tileset pads it and makes its texture in that frame (about 3 ms here, mostly the CPU mip chain), and every landing builds and uploads its visible chunks (about 0.6 ms at 1080p: 27,000 instances of 128 bytes, about 3.5 MB written three times). Proposed: pad the map's tilesets in its landing job, on the worker, and make the padded texture's levels there too, so the landing frame only writes them (or pads when a tileset first loads); and either build a landing's visible chunks within a per-frame budget before the map shows, or give chunks a compact instance (a cell index and a tile slot, about 16 bytes, the shader working out the rest), which would cut the upload about eightfold. A design change across the landing, the padded cache, the graphics upload and the tile shader; not built.
- **On the web the landing job runs inline**, in the frame it is sent: about 5 ms for a 256² four-layer map (natively spawn to landed, frames stepped back to back), and about 80 ms for a 1024² one. A web game should show a loading screen over a big map's landing.
- **Lighting has no spatial index:** every lit frame gathers every map occluder, and each shadowed light loops over all of them, however far. One light over the 1024² cave map costs about 46 µs more than the unlit map. Proposed: index occluders by chunk (they are per chunk already) so a light reads only the chunks its reach meets.
- **A 1024² four-layer map's first build** takes about 80 ms on the worker and 5.5 ms in its landing frame (spawning its 2,900 parts and chains and inserting the colliders): beyond the spec's 256² target, and a hitch a loading screen should cover.
