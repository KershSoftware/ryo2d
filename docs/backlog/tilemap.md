# Tilemaps (Sub-project 6a): speed

Spec: `docs/specs/2026-10-29-tilemap-design.md` (Acceptance, Targets). The 6b feature list is `docs/backlog/tilemap-features.md`.

## 6a: measured

Measured on an Apple M5 Pro (18 cores, Metal, macOS 27.0.1), a shared machine. The record below is from 2026-10-31 between 00:35 and 00:41, after the review's fixes. By then the other work that ran through the evening had ended: three `BanjoRecompiled` processes (a game, rendering on the same GPU) and a Python job. Frames cost less than in the first record, at the same 1-minute load: an empty frame's step read 93 µs, against 147 to 175 µs earlier that night. So numbers from different sessions do not compare. Each fix's before and after numbers come from one session, and each is given with its load or time.

**Method.**
- `benches/tilemap.rs` (its module docs say what each guard does) ran three times on the branch, in the workspace's own `target/` (the bench profile: release with debug info). Each run waited until the 1-minute load was 4 or less. The loads at the runs' starts were 3.79, 3.56 and 3.12, and at their ends 4.03, 3.12 and 3.18.
- Engines run at 1920 x 1080 with 16 px tiles at zoom 1, headless (`HeadlessEngine`) on the GPU.
  - A frame's cost is the wall time of the whole step. The GPU finishes before each step, outside the measure (as `benches/ui_render.rs` does).
  - The static guard's engines step in turn, one frame each. The landing frames are paced at 60 Hz with a spin wait.
- Each verdict holds the median, with the 95th percentile and the maximum printed beside it.
  - The landing's verdict is its slowest frame over the run's median ordinary frame. A run over the limit is measured once more, and the better run is held.
- The one-off timings (ignored tests) ran once at a load of 3.91, in release.

### The targets

| Target | Measured, three runs: median (95th percentile, max) | Verdict |
|---|---|---|
| A static 1024 x 1024 four-layer map adds at most 50 µs of CPU a frame over an empty frame (`static_frame`: the map colliding and casting shadows, the camera flying) | +83.8 (119.3, 348.5), +85.3 (130.2, 467.2), +89.1 (113.9, 1,781.7) µs. Since transform propagation walks only the subtrees that changed (6b, three runs on 2026-11-01, an empty frame's step 93.6, 94.3 and 95.4 µs): +45.3 (91.3, 345.5), +43.8 (84.2, 1,612.7), +45.7 (104.4, 1,189.7) µs | **met** in every 6b run, since propagation walks only the subtrees that changed (missed before: transform propagation) |
| An edit's chunk rebuild within 100 µs: the slowest of the eight edits' medians | 22.6 (50.0, 123.5), 21.8 (78.5, 219.8), 19.9 (49.5, 205.8) µs | **met**, every edit in every run |
| An edit's loop rebuild within 100 µs: the slowest of the eight (each run the serpentine split through its middle row, about 2,000 edges a half) | 65.0 (117.4, 130.7), 63.6 (72.1, 111.5), 62.6 (73.0, 98.8) µs | **met**, every edit in every run (the splits and joins 125.7 to 213.8 µs before the fix, load 8.5) |
| An edit's occluder rebuild within 100 µs: the slowest of the eight | 79.2 (90.9, 96.0), 77.9 (91.7, 99.4), 79.2 (83.5, 87.3) µs | **met**, every edit in every run |
| A 256 x 256 four-layer TMX in base64 and zlib loads off the main thread within 20 ms (with its TSX and PNG, fresh files) | 1,266.6 (1,351.7, 1,351.7), 1,240.7 (1,378.0, 1,378.0), 1,229.2 (1,336.9, 1,336.9) µs | **met** |
| It adds no hitch to the frame it lands in: the slowest landing frame within 0.5 ms of an ordinary frame | the better measurement's slowest: +932.7, +848.6, +972.3 µs. Each time this was the first landing, which makes the padded tileset's texture; the other landings' medians were +488 to +511 µs | **missed**, reported: the first draw (see Limits). The slowest frame read +3,250.7 to +3,353.5 µs before the fixes |
| A frame drawing a static map allocates nothing (`tests/tilemap_alloc.rs`) | Still and panning, the map's frame makes the same allocations of the engine's own as an empty frame: the same count and sites. No GPU-API allocation is made on behalf of the tilemap's code. A padded tileset cache hit makes none | **met** |
| Steady frames create no bind groups, with an image layer now among the map's layers (`steady_frames_create_no_bind_groups`) | none in three steady frames | **met** |

### The static frame (`static_frame`)

The map: 1024 x 1024 cells of 16 px in four layers over one 256 x 256 atlas:
- a background on every cell;
- cave rock from value noise, with a full-cell collision shape, so it collides and casts shadows;
- props on about one open cell in 16;
- front details on one in 64.

It is built in code and landed, then given 120 frames to warm up. Each engine then runs 2,000 frames, the engines interleaved. The camera flies a long loop over the map at 444 to 690 px/s (561 on average). New chunks come into view every few dozen frames, and the frames that build them cost more; the 95th percentiles and maxima include them.

| Each engine, its step over the empty frame's median: median (95th percentile, max) | Run 1 (3.79) | Run 2 (3.56) | Run 3 (3.12) | Its render step over the empty frame's |
|---|---|---|---|---|
| An empty frame: the step (its render step) | 93.0 µs (67.2) | 92.5 µs (67.3) | 93.2 µs (69.3) | |
| The map, colliding and casting shadows (held): 4,096 entities, 2,881 of them children, 2,378 occluder chains | +83.8 (119.3, 348.5) | +85.3 (130.2, 467.2) | +89.1 (113.9, 1,781.7) | +32.4, +33.1, +34.8 µs |
| The map drawn only (no collision, no shadows): 4 children | +32.5 (64.3, 322.0) | +33.5 (80.8, 299.8) | +34.6 (63.3, 1,259.2) | +32.2, +32.7, +34.4 µs |
| Colliding, without shadows: 503 children (collider parts) | +43.7 (79.8, 387.2) | +45.0 (102.0, 654.6) | +46.6 (77.0, 280.1) | +32.6, +33.3, +35.3 µs |
| Casting shadows, without collision: 2,382 children (occluder chains) | +74.0 (109.3, 470.0) | +74.0 (125.2, 844.7) | +77.5 (103.8, 382.2) | +32.5, +32.2, +34.4 µs |
| Its props layer y-sorted | +154.4 (200.7, 489.8) | +157.0 (216.9, 784.6) | +159.7 (196.3, 398.3) | +103.1, +105.1, +106.9 µs |
| Lit by one shadowed light following the camera, over an empty lit frame (130.0, 129.4, 131.2 µs) | +115.0 (170.7, 529.8) | +117.2 (194.1, 723.6) | +119.1 (162.2, 406.5) | +100.2, +102.3, +103.7 µs |

- **Drawing the map costs about 33 µs**, nearly all of it in the render step (+32 to +35 µs). About 16 to 20 chunks of each layer are in view, and each is drawn once per tileset, bound and encoded on its own. On the busier machine of the first record this read +46 to +51 µs.
- **The miss was the map's child entities.** Every collider part and occluder chain is a child entity, and each cost about 17 to 24 ns a frame. The 503 parts added 11 to 12 µs, and the 2,378 chains 40 to 43 µs, so the map with both read 84 to 89 µs.
  - A profile of the map frame (`--profile-time 20 '^static_frame$'`, first record) charged 22% of it to `propagate_transforms`.
  - Once anything had moved (here the camera, in a game a player), each of propagation's three runs a frame walked every entity of every hierarchy. The runs are physics' own schedule, the fixed step's end and `PostUpdate`. Each recomputed only what changed, but visited everything.
- **Fixed in 6b: propagation walks only the subtrees that changed** (`ecs/propagate.rs`). Each run starts from the entities that changed since its last run, each from its parent's stored global transform, so the map's still children are not visited. What is left of them is finding the changed entities, a change-tick check per entity per run. Three runs on 2026-11-01 between 01:27 and 01:28, the 1-minute load 4.64, 4.10 and 4.25 at their starts, nothing else heavy running, an empty frame's step 93.6, 94.3 and 95.4 µs (as in the record above), each show's step over the empty frame's median:

| Show | Run 1 | Run 2 | Run 3 |
|---|---|---|---|
| The map, colliding and casting shadows (held) | +45.3 (91.3, 345.5): **met** | +43.8 (84.2, 1,612.7): **met** | +45.7 (104.4, 1,189.7): **met** |
| Drawn only | +35.2 (84.5, 1,120.7) | +34.3 (78.5, 330.9) | +34.1 (96.1, 2,020.0) |
| Colliding, without shadows | +38.9 (82.2, 336.3) | +38.7 (78.8, 329.1) | +39.8 (96.9, 894.6) |
| Casting shadows, without collision | +41.8 (82.1, 6,519.8) | +40.5 (78.4, 4,926.7) | +42.2 (95.5, 693.0) |
| Y-sorted | +115.9 (171.3, 480.7) | +115.6 (165.5, 383.6) | +119.3 (184.7, 807.5) |
| Lit, over an empty lit frame (131.3, 134.5, 134.8 µs) | +77.7 (143.8, 398.2) | +77.1 (134.7, 493.0) | +79.6 (160.5, 454.0) |

  - So the parts and chains add about 10 µs together (9.5 to 11.6), against 51 to 55 µs. The held margin is 4.3 to 6.2 µs.
  - A first run, earlier that night with an empty frame of 91.7 µs, read +41.7 µs (67.0, 368.6). The same session's run before the change read the map at +105.1 µs (242.3, 1,609.9) and drawn only at +49.1, but the machine was slower then (nothing else heavy ran): an empty frame's step read 130.3 µs, and the maps' render steps about 15 µs dearer, so it does not compare with the runs after.
- **A y-sorted layer** draws each visible row of each chunk as an item of its own: one per row, chunk, tileset and camera. It costs about another 71 µs here; there is no target for it.
- **Lit frames** pay for every occluder in the map: about 30 µs more than the unlit map over the same light (see Limits).

### The edit (`edit`)

The map: 1024 x 1024 cells in four layers.
- The ground holds 3 x 3 islands every 8 cells: about 16,000, packed into about 1,000 chunk colliders, each with one occluder chain or more.
- Two loops sit each in its own row of 4 x 1 chunks:
  - a jagged skyline, 128 columns up to 24 cells high, whose column 99 holds only its bottom cell (a neck);
  - a one-cell serpentine filling the four chunks.

Eight edits, each set and cleared again with an ordinary frame between edits, as a game edits:
- a bump on each loop: 400 edits at (64, 20) and (64, 64). The loop stays one.
- a split and a join on each loop, 200 edits each: the skyline's neck (99, 31), and the middle of the serpentine's first row (64, 65). Clearing the cell splits the loop in two, and setting it joins it again.
- the serpentine split and joined through its middle row (64, 79): halves of about 2,000 edges each, the worst split a loop of four chunks can have.

The held parts, median (95th percentile, max):

| Edit: part | Run 1 | Run 2 | Run 3 |
|---|---|---|---|
| Skyline, a bump (64, 20): the chunk rebuild | 22.6 (50.0, 123.5) | 20.6 (45.9, 118.7) | 19.8 (36.7, 91.0) |
| Skyline, a split (the neck, 99, 31): the chunk rebuild | 22.5 (38.0, 90.2) | 21.8 (78.5, 219.8) | 19.9 (49.5, 205.8) |
| Skyline, a join (the neck): the chunk rebuild | 22.5 (42.0, 6,526.2) | 21.0 (80.4, 189.9) | 19.7 (46.2, 110.0) |
| Serpentine, a bump (64, 64): the chunk rebuild | 21.2 (31.9, 60.8) | 19.0 (44.3, 2,218.4) | 18.1 (33.0, 96.3) |
| Serpentine, a split (its first row, 64, 65): the chunk rebuild | 21.1 (86.7, 163.0) | 19.0 (33.7, 50.7) | 16.5 (31.8, 91.2) |
| Serpentine, a join (its first row): the chunk rebuild | 20.5 (82.6, 181.9) | 18.7 (34.0, 48.2) | 17.0 (35.9, 2,500.4) |
| Serpentine, a split through its middle row (64, 79): the chunk rebuild | 20.5 (106.0, 206.1) | 18.9 (30.4, 61.2) | 18.4 (36.8, 74.5) |
| Serpentine, a join through its middle row: the chunk rebuild | 20.8 (106.7, 188.3) | 19.2 (30.8, 67.3) | 18.4 (37.1, 85.6) |
| Skyline, a bump: the loop rebuild | 56.2 (65.0, 119.0) | 54.8 (62.3, 108.6) | 54.2 (61.5, 103.8) |
| Skyline, a split: the loop rebuild | 58.6 (67.7, 89.0) | 60.1 (84.7, 101.2) | 56.7 (71.9, 93.5) |
| Skyline, a join: the loop rebuild | 58.0 (63.5, 79.0) | 59.2 (84.3, 98.9) | 56.6 (68.6, 86.9) |
| Serpentine, a bump: the loop rebuild | 51.3 (53.8, 66.5) | 50.5 (56.5, 68.9) | 50.0 (54.0, 62.7) |
| Serpentine, a split: the loop rebuild | 56.6 (72.2, 84.0) | 55.1 (59.5, 64.2) | 54.2 (58.4, 76.5) |
| Serpentine, a join: the loop rebuild | 52.6 (69.0, 78.6) | 51.8 (55.8, 62.3) | 51.0 (55.0, 66.1) |
| Serpentine, a split through its middle row: the loop rebuild | 65.0 (117.4, 130.7) | 63.6 (72.1, 111.5) | 62.6 (73.0, 98.8) |
| Serpentine, a join through its middle row: the loop rebuild | 62.2 (111.8, 222.2) | 60.9 (65.2, 90.7) | 60.2 (70.8, 98.1) |
| Skyline, a bump: the occluder rebuild | 42.6 (46.8, 82.0) | 42.9 (45.9, 64.1) | 42.7 (45.8, 66.3) |
| Skyline, a split: the occluder rebuild | 78.5 (83.2, 94.5) | 77.8 (87.8, 99.0) | 78.9 (85.4, 99.2) |
| Skyline, a join: the occluder rebuild | 78.6 (82.8, 102.0) | 77.9 (91.7, 99.4) | 78.5 (84.0, 96.4) |
| Serpentine, a bump: the occluder rebuild | 74.3 (81.8, 243.5) | 73.8 (82.4, 258.4) | 73.0 (82.4, 236.4) |
| Serpentine, a split: the occluder rebuild | 78.4 (88.0, 97.9) | 74.2 (82.0, 87.2) | 79.2 (83.5, 87.3) |
| Serpentine, a join: the occluder rebuild | 75.5 (85.2, 92.0) | 73.4 (78.7, 87.3) | 74.2 (82.3, 90.7) |
| Serpentine, a split through its middle row: the occluder rebuild | 79.2 (90.9, 96.0) | 77.1 (82.5, 92.9) | 79.2 (83.8, 102.9) |
| Serpentine, a join through its middle row: the occluder rebuild | 79.0 (87.1, 104.8) | 76.7 (82.0, 86.7) | 79.1 (82.9, 86.8) |

The loop rebuild's two parts, back to back, and the whole edit frame, medians of runs 1, 2 and 3 (no target):

| Edit | Collision sync (the chunk classified again across four layers) | Tile collider rebuild and collider sync (retrace, reshape, the Rapier shape) | Back to back, no frame between (data in cache): loop rebuild; occluder rebuild | The edit frame's whole step over an ordinary frame's |
|---|---|---|---|---|
| Skyline, a bump | 20.0, 19.3, 18.8 µs | 36.0, 35.8, 35.3 µs | 49.0, 49.1, 49.0; 39.3, 39.4, 39.4 µs | +266.5, +267.5, +265.5 µs |
| Skyline, a split | 19.6, 19.0, 18.5 µs | 39.0, 41.8, 38.2 µs | 51.0, 51.1, 51.2; 67.6, 67.8, 68.1 µs | +272.6, +276.8, +273.4 µs |
| Skyline, a join | 19.6, 19.3, 18.5 µs | 38.4, 40.8, 38.1 µs | 50.7, 50.9, 51.0; 67.5, 67.6, 67.8 µs | +271.7, +276.1, +272.7 µs |
| Serpentine, a bump | 18.5, 17.2, 17.5 µs | 32.8, 32.9, 32.6 µs | 45.1, 44.9, 44.9; 66.5, 66.5, 67.0 µs | +264.9, +268.7, +276.5 µs |
| Serpentine, a split | 18.5, 17.5, 17.1 µs | 38.0, 37.2, 37.1 µs | 50.4, 49.2, 49.2; 69.0, 67.0, 67.3 µs | +273.2, +274.2, +279.3 µs |
| Serpentine, a join | 18.5, 18.1, 17.1 µs | 34.1, 33.7, 33.9 µs | 47.0, 46.0, 46.0; 68.3, 66.7, 67.0 µs | +270.1, +273.9, +279.3 µs |
| Serpentine, a split through its middle row | 18.5, 18.1, 17.1 µs | 46.5, 45.7, 45.5 µs | 54.5, 54.6, 54.6; 67.3, 67.0, 67.4 µs | +275.2, +277.3, +272.2 µs |
| Serpentine, a join through its middle row | 18.5, 17.5, 17.1 µs | 43.7, 43.2, 43.1 µs | 52.1, 52.2, 52.4; 67.1, 67.0, 67.5 µs | +272.6, +277.6, +272.0 µs |

- **A frame between edits matters.**
  - The first runs read the loop rebuild at 198.7 µs (skyline) and 547.0 µs (serpentine) with a frame between edits, against 53 and 118 µs back to back. The retrace walked every edge of the loop through the outlines' map of points (about 200,000 points on this map), and once a frame has passed each step is about a cache miss.
  - The physics module's own timing (`one_tile_edit_timing`, edits back to back) never saw this. Fixed: see the fixes.
- **The splits and joins** are spliced as the bumps are (see the fixes). The middle cut, whose smaller half's 2,000 edges each take the other loop's id, adds about 8 µs to the loop rebuild. Its 95th percentile in run 1 (117.4 µs) is the only one over 100 µs.
- **The edit frame's whole step** paid for the edit's new occluder entities and reshaped collider through transform propagation. The edit changes entities, so propagation's runs walked the map's 17,000 children (34% of the `edit` bench's profile in the first record). Since propagation walks only the subtrees that changed (6b), the edit frame's whole step reads +16.2 to +23.9 µs over an ordinary frame (one run on 2026-11-01, an ordinary frame's step 152.8 to 155.5 µs): the skyline +16.2, +22.6 and +22.7 µs, the serpentine +21.9, +21.9 and +23.6 µs, and through its middle row +22.6 and +23.9 µs. Every held part met its target in that run. This is not part of the targets.

### The load and the landing (`load`)

Twenty loads of fresh files each (map, tileset and image under new names, the asset workers started by an untimed load first), pumping on the main thread with a yield between pumps:

| | Run 1 | Run 2 | Run 3 |
|---|---|---|---|
| Load: TMX, TSX and PNG, `load` to ready with its dependencies (held) | 1,266.6 µs (1,351.7, 1,351.7) | 1,240.7 µs (1,378.0, 1,378.0) | 1,229.2 µs (1,336.9, 1,336.9) |
| The main thread's pumps meanwhile | 739.9 µs | 756.4 µs | 747.0 µs |
| A level change (the tileset loaded already) | 1,217.7 µs | 1,224.9 µs | 1,239.1 µs |

The landing: an engine at 60 Hz loads and spawns a fresh copy of the map every 40 frames (its tileset held loaded) and despawns it 32 frames later, twenty times a run. Each map landed one frame after its spawn.

| Over the run's median ordinary frame; each run's two measurements | Run 1 | Run 2 | Run 3 |
|---|---|---|---|
| The median ordinary frame | 131.7; 134.8 µs | 131.5; 139.2 µs | 137.7; 131.0 µs |
| The asset landing (the frame the map's asset lands in, which sends its landing job): median (max) | +10.4 (306.2); +12.2 (177.8) µs | +11.9 (248.2); +5.2 (276.3) µs | +0.0 (260.0); +9.1 (283.1) µs |
| The first map landing (makes the padded tileset's texture) | +1,064.0; +932.7 µs | +1,015.2; +848.6 µs | +1,044.7; +972.3 µs |
| The other map landings: median (95th percentile, max) | +487.5 (801.2, 801.2); +494.8 (560.7, 560.7) µs | +498.5 (609.6, 609.6); +495.3 (809.1, 809.1) µs | +511.1 (908.2, 908.2); +499.1 (726.8, 726.8) µs |
| The frame after: median (max) | +17.2 (217.1); +15.5 (121.9) µs | +21.0 (90.7); +11.0 (124.8) µs | +43.2 (327.8); +18.4 (216.6) µs |
| Held: the slowest landing frame, the better measurement | +932.7 µs | +848.6 µs | +972.3 µs |

- **The first landing** no longer pads the tileset. Its landing job pads it on the worker and prepares the upload, mip chain included (see the fixes). The landing frame creates the padded texture and writes its levels. That costs it about 0.35 to 0.58 ms more than a later landing (0.5 to 0.7 ms on the busier machine).
- **Every later landing** draws the map for the first time. About 27,000 tiles are in view at 1080p. Each is a 128-byte instance, built, copied into the chunk arena and uploaded: about 3.5 MB. The landing's entities, colliders and chains are spawned too.
  - A profile on the busier machine split the cost over an ordinary frame as follows:
    - the tile drawer's collect, +207 µs, of which building the chunk instances was 99 µs;
    - the batch's prepare, +258 µs: the arena's `write_buffer` copying about 2.1 MB of chunk instances;
    - the rest, about 120 µs.
  - This record reads the median at the limit (+488 to +511 µs) and the maxima over it.
- **The asset landing frames' maxima** (+178 to +306 µs, under the limit) were not profiled. On the busier machine they read +3 to +138 µs over a median ordinary frame three times as long, so the two are not comparable.

### The fixes made in this task

| Fix | Before | After |
|---|---|---|
| **Outline splice** (`physics/tiles/outline.rs`). An edit's retrace copies the dropped loops' corners up to the points whose edges changed, and looks up only those points; each unchanged stretch is copied in one go. Each resulting loop keeps the id of the dropped loop it holds the most of. With several dropped loops (a join) or several results (a split), the edges that change loops take the new id. An edit with doubled edges at a changed point still takes the whole trace. | Loop rebuild with a frame between edits (load 3.6): a bump on the skyline 198.7 µs, on the serpentine 547.0 µs. Splits and joins, still traced whole after the first splice (load 8.5): skyline 129.3 and 125.7 µs, serpentine 213.8 and 206.4 µs. Back to back in `one_tile_edit_timing`: 30 µs (retrace 23) and 88 µs (71) | This record: bumps 50 to 56 µs, splits and joins 51 to 60 µs, the middle cut 60 to 65 µs. Back to back 9.1 µs (retrace 1.75) and 19.7 µs (3.9). A first generalised splice that looked up and labelled edge by edge read 80 to 100 µs for all of these (busier machine, 00:10 to 00:17). The stretch copies brought it to 49 to 59 µs in the same period (load 9.9) |
| **Occluder chains** (`tilemap/occluders.rs`, `chains`). Each point's edges are a run of the edges sorted by start, found through one map made at its full size. There is no list per point, no map grown from empty, and no chain made for every start tried. The chains are the same: checked against the old function on 20,000 random edge sets. | Serpentine occluder rebuild 121.5 µs (load 3.6). Of that, `chains` took 48.5 µs for a dense chunk's 992 edges | 86.6 to 88.5 µs in the first record; 73 to 79 µs in this one |
| **Paddings kept** (`tilemap/padded.rs`). A padded tileset is kept for 120 frames after a map last drew it (as an undrawn chunk is), so a level respawned or changed with the same tileset draws with it | Each landing padded again: the map landing's median +2.5 to +2.7 ms (load 10) | The later landings: medians +562 to +621 µs in the first record; +488 to +511 µs in this one |
| **Padding off the landing frame** (`tilemap/spawn.rs`, `padded.rs`, `render/tiles.rs`, `render/batch/gpu.rs`). A landing job pads the tilesets its map needs and no padding holds yet, and prepares each padded image's upload with its mip chain (`Image::prepare_upload`). The landing frame adopts them. The padded cache is shared (`PaddedCache`, behind a lock) rather than taken out of the world each frame. The tile pipeline is made with the batch, not by the first draw. A re-pad of a tileset already drawn (its pixels changed) stays in the frame. | The first landing: +3,250.7 to +3,353.5 µs (the first record). In it the CPU mip chain took about a quarter, and the first tile pipeline about 0.85 ms | Padding in the job alone: +1.67 to +2.10 ms; with the pipeline made with the batch: +1.18 to +1.32 ms (loads 4 to 12). The busier machine's record runs (00:10 to 00:17): +985.2 to +1,067.0 µs. This record: +848.6 to +972.3 µs |
| **An embedded tileset's padding held by its address** (`tilemap/padded.rs`). An embedded tileset is keyed by its `Arc`'s address. A kept padding (or a refusal) holds a `Weak` to it, so while the padding is kept, no other embedded tileset can take that address and be given its padding | A tileset freed while its padding was kept left its address free for another, which could then draw with the first's padding | The address stays taken while the padding is kept; `a_gone_embedded_tileset_s_address_is_held_while_its_padding_is_kept` |
| **No allocation of the engine's own for a static map's frame.** Transform propagation keeps its walk's stack between runs (`ecs/propagate.rs`). The draw list sorts unstably (`render/draw_list.rs`): each item's sequence is its own, so the order is the same. A chunk of one tileset skips its stable sort (`render/tiles.rs`). | The map's frame made 248 allocations of the engine's own against an empty frame's 229: 18 for propagation's stack and 1 for the sort | The same allocation sites as an empty frame |

### The physics bench (Task 19's floor tie)

`cargo bench --bench physics -- characters` ("100 characters on tiles"), from the first record. There were three alternated rounds of the build before the floor tie (877700c5^'s `character.rs` swapped in place) and after it. Each run started at a load of 4 or less (3.63 to 3.99).
- Before: 639.6, 643.6 and 562.5 µs.
- After: 670.5, 585.5 and 631.4 µs.
- The medians of the rounds were 639.6 and 631.4 µs (-1.3%). The rounds swing by ±60 µs either side.

The guard (no slower than before by more than 5%) is **met**. The review's fixes do not touch a map without edits.

### The carries from earlier tasks

- **Static frame (Task 17's demo: 60 to 67 µs of render CPU in the dev profile at 960 x 600, the camera still):** in release at 1920 x 1080 with the camera flying, the map drawn only adds 32.5 to 34.6 µs (its render step +32.2 to +34.4 µs), and with collision and shadows 83.8 to 89.1 µs (above).
- **Per-frame padded-cache lookups:** a hit costs 0.04 µs for an atlas tileset and 7.96 to 8.12 µs for a 1,000-tile collection (its stamp reads every tile's image version). There is one a tileset a frame.
- **One draw per (chunk, tileset, camera), and per row on a y-sorted layer:** a y-sorted props layer costs the frame about 71 µs more (above).
- **Steady unlit frames +0.05 to +0.08 ms on a dense 256² map with 1,024 parts and chains (Task 11):** on the 1024² cave map, 503 collider parts add 11 to 12 µs and 2,378 chains 40 to 43 µs to a frame whose camera moves (above).
- **Collision edit (Task 10: 37 µs skyline, 87 µs serpentine; the one-chunk reclassify about 18 µs more):** measured in the engine with a frame between edits, the loop rebuilds take 50 to 65 µs, of which the collision sync is 17 to 20 µs (above). `one_tile_edit_sync_timing` (back to back, a 1024² four-layer map) reads the sync at 18.6 µs (max 41.9).
- **Occluder edit (Task 11: 45 to 46 µs median, 192 to 275 µs max):** the skyline bump 43 µs (max 64 to 82), the splits and joins and the serpentine's dense chunk 73 to 79 µs (max 86 to 258). `one_tile_edit_sync_timing` reads 42.2 µs (max 237.5).
- **The IntGrid table rebuilt on every edit step:** not measured on its own. The edit map has no IntGrid layer. The table holds an entry per IntGrid value name and override (a few), and it is rebuilt, with a small allocation, on the steps whose `TileLayerData` changed.
- **The 1024² four-layer first build (Task 11: about 75 ms in the step and frame after spawn):** spawn to landed takes 69.9 to 73.6 ms with frames stepped back to back (the landing job on its worker); the landing frame takes 4.3 to 5.1 ms, and the frame after 0.2 to 0.3 ms. See Limits.
- **The web's inline landing:** a native bench cannot run it. The 256² map's landing job, spawn to landed with frames stepped back to back, takes 4.6 to 4.9 ms, which the web pays in the frame it is sent. See Limits.
- **Idle step (Task 10: 8 µs at 16,300 parts):** `one_tile_edit_timing`'s fixed step with physics and no edit reads 3.8 to 4.1 µs on that grid (1,021 parts). `one_tile_edit_sync_timing` reads a 1024² four-layer map's idle fixed step at 23.8 µs, its idle collision sync at 0.08 µs and its idle occluder sync at 0.08 µs.
- **Lit frames gather every map occluder (`engine/extract.rs`), and each shadowed light loops over all of them:** one shadowed light over the 1024² cave map (2,378 chains) costs 115.0 to 119.1 µs over an empty lit frame, against the unlit map's 83.8 to 89.1 µs. See Limits.
- **Wang `best_tile`:** 15.7 µs a call at 255 tiles with every flip (`best_tile_timing_on_a_255_tile_set`), as Task 12 measured.
- **Hot reload:** `reload_timing` on a 1024² four-layer map.
  - Applying a one-tile save takes 0.99 to 2.34 ms (the first, cold, is the slowest), then 36 to 65 µs for the next step and `PostUpdate`.
  - A save of every chunk takes 1.38 ms to apply, then 76.9 ms.
- **The character controller's floor tie (Task 19):** above.

## Limits

- **Transform propagation still scans every entity for changes.** It walks only the subtrees that changed (`ecs/propagate.rs`), but finding them is a change-tick check per entity per run, about 0.7 ns: about 3 µs a run for the 1024² cave map's 4,096 entities, three runs a frame. Bigger maps, and streamed chunks, add to it.
  - **A run that changes many entities pays the scan on top.** Where the changed subtrees cover the hierarchy, the walk visits every entity as the full walk would, plus the scan: `benches/foundation.rs`'s 100 roots moved over 10,000 entities cost 77.4 µs against 73.5 µs before (+5.3%, the same session).
  - **A fifth changed walks everything.** Once the changed entities are a fifth of those with a `Transform` (and more than 64), a run walks every hierarchy, as before 6b, the map's children included: there that costs about what the changed subtrees would (2,000 leaves moved of 10,000 entities: 83 µs through the changed subtrees, about 81 through the full walk). The fifth was measured on leaves two deep; deeper changed entities cost the changed subtrees' walk more each, so for them the crossover comes earlier.
  - Proposed, if the scan shows in a profile: keep a static map's parts (collider parts, occluder chains) out of the scanned set, which changes what `TilePart` and `MapOccluder` carry.
- **Drawing a big map is most of its frame:** the map drawn only adds 34.1 to 35.2 µs, of the static map's 43.8 to 45.7 µs (three runs). Proposed, for the margin: **merged contiguous chunk draws.** Neighbouring chunks of a layer whose instances lie next to each other in the arena could share one draw, instead of one draw and its encoding a chunk.
- **The landing frame draws the map for the first time** (the landing miss). What is left after the fixes:
  - **The first landing of a tileset** creates its padded texture and writes its levels in that frame: about 0.35 to 0.58 ms more than a later landing.
  - **Every landing** builds and uploads its visible chunks: about +0.5 ms at 1080p (27,000 instances of 128 bytes). The busier machine's profile put about 0.2 ms in the drawer (0.1 ms of it building the instances), about 0.26 ms in copying them into the arena's buffer, and about 0.12 ms elsewhere.
  - **Instances built on the worker were weighed and not built.** They can save at most the 99 µs the build takes, which leaves the copy, the upload and the first texture: the guard would still miss.
  - **Proposals that keep the map whole on its landing frame (R16):**
    - Write the padded texture over the frames the landing job already takes (about 5 ms, several frames at 60 Hz), a level or a band of rows a frame, so it is whole before the map lands.
    - Give chunks a compact instance: a cell index and a tile slot, about 16 bytes, the shader working out the rest. That cuts the copy and the upload about eightfold. Deferred by the ruling.
    - Give the chunk arena a mapped staging buffer that the build writes straight into, instead of copying with `write_buffer`.
  - Each is a design change across the landing, the graphics upload and the tile shader; none is built.
- **A split relabels its smaller part edge by edge.** Each edge that moves to the other loop's id costs a lookup. The middle cut of a four-chunk serpentine (about 2,000 edges) adds about 8 µs, and its loop rebuild reads 60 to 65 µs, but a loop spanning many more chunks, split into long halves, pays in proportion.
  - An edit with doubled edges at a changed point (a broken shape's) still walks the whole of every loop it dropped.
  - Proposed: labels by range (each loop's edges as runs in a per-loop order, so a split moves whole runs), or relabelling lazily on the next trace that meets an edge.
- **On the web the landing job runs inline**, in the frame it is sent: about 5 ms for a 256² four-layer map (natively spawn to landed, frames stepped back to back), and about 70 to 90 ms for a 1024² one. A web game should show a loading screen over a big map's landing.
- **Lighting has no spatial index.** Every lit frame gathers every map occluder, and each shadowed light loops over all of them, however far away. One light over the 1024² cave map costs about 30 µs more than the unlit map. Proposed: index occluders by chunk (they are per chunk already), so a light reads only the chunks its reach meets.
- **A 1024² four-layer map's first build** takes about 70 ms on the worker and 4.3 to 5.1 ms in its landing frame, which spawns its 2,900 parts and chains and inserts the colliders. This is beyond the spec's 256² target, and a hitch a loading screen should cover.
- **The chunk arena holds at most the GPU's largest buffer:** 256 MiB under the engine's limits, 2,097,152 tile instances of 128 bytes, the chunks in view with those drawn in the last 120 frames. A view meeting more (a 1024² four-layer map seen whole at about zoom 0.12 at 1080p, or a minimap camera over a big map) first frees the chunks drawn least recently, then draws only the chunks it meets first, and the dev log warns once. Proposed: the compact instance of about 16 bytes (eight times the tiles in the same buffer), or several arena buffers.
- **A tileset's padded image must fit one texture:** 8,176 texels a side under the engine's limits (the GPU's 8,192, less the atlas's border). That is about 3,800 tiles of 64 px, 15,876 of 32 px, or a collection of 196 images up to 256 px (36 up to 512 px). A larger one draws nothing, warned. Proposed: cells per size class, or several padded pages per tileset.
- **Enabling a disabled map rebuilds its collision and shadows on the main thread, in that frame's fixed step and `PostUpdate`.** A disabled map takes away its colliders and empties its shadow chains, and enabling it builds every grid and every chunk's chains again from its tiles, as a first build, but not on the builder thread. Not measured on its own: the same work off the frame (a landing job) takes 4.6 to 4.9 ms spawn to landed for a 256² four-layer map, which bounds it, and about 70 ms for a 1024² one, as does `reload_timing`'s main-thread rebuild of every chunk (76.9 ms). So parking big levels with `Disabled` and enabling them costs a visible hitch. Proposed: keep a disabled map's grids, traces and chains and disable their parts (physics already skips a disabled collider), or send the rebuild to the builder thread as a landing is.
- **Collision is capped at 8,192 cells a side.** A colliding group longer than that on either side gets no colliders (the dev log warns once, naming its layers), so a long runner level of 10,000 x 64 cells does not collide. Before this sub-project the cap was on area (2^26 cells), and that level collided. The side cap keeps every outline point of a grid (11,520 points a cell) in an `i32`. Proposed: outline coordinates relative to their chunk, so a side's length no longer bounds them and the area cap can come back.
- **A map's tile layers hold at most 2^27 cells together** (512 MiB of cells), and an LDtk project's levels the same together (an IntGrid layer counting twice, empty layers counted), checked from the layers' sizes before any is decoded or allocated. It loads the spec's maps (an 8192² layer, a 1024² map of many layers) and keeps a small hostile file from asking for gigabytes; a landing copies the grids once more, so a map at the cap peaks near 1 GiB. A project of many big levels, all held at once, meets it first. Proposed: LDtk levels loaded one at a time (a level asset each), and grids allocated lazily by chunk.
- **An LDtk layer splits into 16 layers at most** for its stacked, offset and translucent tiles: each is a grid the size of the level and a draw a chunk. Past 16 the tiles left over draw in the nearest sub-layer, at a nearby offset or depth, warned once a layer. LDtk 1.5.3's samples ask for up to 44 in a rule-heavy layer, which then snaps 28 tiles. Proposed: sub-layers sized to their tiles' bounds, or chunks holding several tiles a cell.
- **Editor features drawn otherwise, warned once a file:** Tiled's tile render size "grid" and fill mode, collection tiles drawing part of their image, render orders other than right-down, top-down object layers whose tile objects overlap out of order by y, polylines among collision shapes; LDtk's entity tile render modes other than stretching, and parallax scaling. Each could be read: sub-rectangles as a tile's source rectangle in the padding, render order and draw order as a sort, the render size as a scale in the chunk build.
- **The keep window holds a padded tileset's memory** for 120 frames after the last map using it goes. A level change with a different tileset briefly holds both paddings, and their textures (see the tilemap module notes).
