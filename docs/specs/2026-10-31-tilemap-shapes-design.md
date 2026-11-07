# Tile Map Shapes and Infinite Maps (6b) Design

## Overview

Sub-project 6a (`docs/specs/2026-10-29-tilemap-design.md`) gave Ryo2D professional orthogonal tile maps from Tiled and LDtk: registry assets with hot reload, chunked drawing, collision and shadows from the tiles, objects as entities, run-time edits with Wang terrain, worlds. Every Tiled map whose grid is not square, and every infinite map, is refused today with an error naming 6b.

6b makes the engine a complete tile map suite for every grid Tiled draws and for maps without edges:
- isometric, staggered isometric, hexagonal and oblique grids, from Tiled files and built in code;
- floors on any grid, so characters walk on and behind raised ground;
- infinite maps, from Tiled's chunked files and grown at run time, with the engine streaming chunks around the player;
- pathfinding on every grid shape.

It is not shaped around one game. The owner's bar: a set of tools a professional would expect, with nothing someone could point to and call unprofessional. The research behind this design:
- `docs/research/2026-10-31-tile-shapes-survey.md`: how Godot, Unity, Bevy's crates, Phaser and others handle these maps, and what developers complain about;
- `docs/research/2026-10-31-tile-shapes-formats.md`: what Tiled 1.12.2 writes and how it draws each grid, measured from files and renders Tiled made;
- `docs/research/2026-10-31-tile-shapes-math.md`: the grid maths;
- `docs/research/2026-10-31-tile-shapes-seams.md`: every place 6a's code assumes a square, bounded grid.

## Goals

1. **Every Tiled grid.** Maps with `orientation` isometric (any tile ratio), staggered (both stagger axes and indices), hexagonal (pointy and flat, both indices, any `hexsidelength`) and oblique (`skewx`, `skewy`) load, in both formats, and behave exactly as Tiled shows them. A map can also be built in code with any of these grids.
2. **Everything 6a does, on every grid.**
   - **Drawing:** tiles placed where Tiled places them, in Tiled's order within a layer. Hexagonal tiles keep Tiled's 60° and 120° rotations as well as their flips.
   - **Objects:** placed where Tiled shows them. Isometric and oblique object coordinates convert from Tiled's stored space on load.
   - **Collision and shadows:** from the tiles, with an edit retracing only what it touched, as in 6a.
   - **Also:** run-time edits, y-sorting with characters, class tiles, worlds and hot reload.
   - **Terrain painting:** on isometric and staggered maps, matching Tiled's brush.
3. **Characters sort correctly with tiles.** On every grid, a character walks in front of and behind walls, trees and props, including tiles taller than their cell. This is the most reported pain point in every engine surveyed, so it has its own tests.
4. **Floors.** A layer can belong to a floor (raised ground, an upper storey, a bridge). Characters on a floor:
   - draw in front of and behind that floor's tiles and the floors around it correctly;
   - collide only with their floor;
   - change floors through stairs and ramps the designer marks.

   Floors work on every grid, not only isometric.
5. **Grid tools for games.** For every grid:
   - world to cell, cell to world, and the cell under the mouse (the nearest hexagon on hex maps);
   - neighbours and distance;
   - lines, ranges and rings on hex grids.
6. **Infinite maps.**
   - Tiled's infinite maps load: chunks at any coordinates, negative ones included, in every encoding, on every grid.
   - A map can also grow at run time: writing a tile anywhere creates its chunk, and drawing, collision and shadows follow.
7. **Streaming endless worlds.**
   - A game gives a map a generator. The engine fills chunks the first time a camera or a chosen entity comes near them, off the main thread, so the frame never hitches.
   - Chunks far from every camera and chosen entity unload. A chunk the game or the player changed is handed back to the game to save, and the game can supply it again when it is needed, before the generator would run.
   - A Tiled infinite map can stream too: its saved chunks are the hand-made part of the world, and the generator fills everything else.
8. **Pathfinding on every grid.** The navigation grid works on isometric, staggered and hexagonal maps (six neighbours on hex), built from a map's collision like today. It works over the loaded part of an infinite map. Paths cross floors: a character finds a path that climbs stairs to another floor and comes back down.
9. **Fast big maps.** The engine-wide fix 6a left in the backlog: transform propagation skips subtrees where nothing changed, so a big static map meets 6a's frame target. Streaming builds on this, since streamed chunks keep adding collider and shadow entities.
10. **Proof.**
    - A demo with real CC0 art shows each goal, including speed pages with the targets below measured.
    - The `ryo-tilemap` skill teaches all of it, and the other skills that touch maps are updated.

## Non-goals (6b)

- **Terrain painting on hexagonal maps.** Tiled's terrain brush does not support hex maps (Tiled issue #2824, open since 2020: the brush assumes four corners, and a hexagon has six). Without an editor to author hex terrain sets, a run-time feature would have nothing to read. Logged for later.
- **Hexagonal generators in the procgen crate.** Procgen's generators stay square. Their grids convert to square, isometric and staggered maps, which share the square lattice.
- **LDtk changes.** LDtk is orthogonal only and has no infinite maps.
- Running Tiled's automapping or LDtk's auto-layer rules at run time, saving maps back to the editors, or a tile editor inside the engine.
- **Loading and unloading a world's levels automatically.** Streaming here is chunks within one map; levels of a `.world` stay the game's call, as in 6a.
- **The first-landing frame miss** from 6a (`docs/backlog/tilemap.md`). It stays in the backlog.

## Anti-goals (the professional bar)

- A map that loads but draws, collides, sorts or places objects differently from what Tiled shows.
- **A dead option:** an orientation, flag or setting the engine accepts but does not implement. Anything 6b does not do keeps being refused with a clear error.
- A frame cost that grows with the map's size, or the size of the streamed world, rather than with what is on screen.
- A streaming hitch: generating, landing or unloading chunks must never stall the frame on native platforms.
- Memory that grows without bound in an endless world.
- Characters that sort wrongly with tall tiles or across floors.
- Seams or bleeding between tiles on any grid, at any zoom.

## Decisions

### The owner's decisions (2026-10-31)

The owner wants a professional suite rather than features for one game. They agreed to the picture proposed after the research:
- every Tiled grid;
- everything 6a does, on each grid;
- grid tools for games;
- pathfinding on hex and isometric grids;
- a demo.

They also decided:
- Infinite maps include maps grown at run time, not only Tiled's files.
- The engine streams chunks: a game generator fills chunks near cameras or chosen entities, far chunks unload, and changed chunks are handed back to save.
- Floors are in: elevation per layer, drawing that sorts across floors, collision per floor, and stairs and ramps.
- Terrain painting on hexagonal maps is left out.
- 6b starts with the transform propagation fix 6a left in the backlog.

After the design's review the owner decided:
- **Pathfinding crosses floors.** It uses one graph linked at stairs, so characters can path up and down by themselves.
- **6b merges in stages.** Each stage has its own plan, review and hand-play, and merges when it is done, leaving main working with its docs and skills current. The stages are below.

### My calls (the owner may revisit them in review)

- **One grid geometry, used everywhere a cell meets a pixel.** A map carries its grid (orientation, stagger, hex side, skew), and every layer copies it. One geometry type answers every grid question:
  - cell to layer pixel, and back;
  - a cell's box, its outline polygon, its neighbours;
  - the draw order inside a layer;
  - a chunk's bounds.

  6a writes `cell * cell_size` in more than fourteen places (the seams note lists them). Each goes through the geometry, so none stays square by accident.
- **Collision baked into the outline points, not a sheared parent.** Outlines are traced in the grid's own lattice, and their points are mapped to layer pixels before colliders are built. Putting colliders under a sheared parent transform would rebuild every collider on the main thread (physics builds shapes ahead only for unscaled colliders) and give one-way tiles wrong normals.
  - Isometric, staggered and oblique maps trace in the square lattice, as 6a does, and map the points through the grid. A staggered map is an isometric lattice indexed differently.
  - Hexagonal maps get a hex lattice in the outline tracer: three border units owned per cell, points on a hex lattice, packing by chunk. The tracing and splicing logic is reused. A hex region has no corner-touching ambiguity (every vertex meets three cells), so loops are simpler than on squares.
- **Tile shapes on non-square grids.** Collision shapes drawn per tile in Tiled's collision editor are in the tile image's pixels, and convert to the cell's space through the grid.
  - A shape covering the whole cell (the diamond on isometric maps, the hexagon on hex maps) merges into outlines as a full tile does.
  - Other shapes stay authored shapes.
  - One-way tiles are a platformer feature for orthogonal maps. On other grids, a `one_way` tile collides both ways, and the dev log warns once per map.
- **Sorting with characters.**
  - A tile or entity sorts by its ground position: where it stands, before any floor's height lifts it on screen (see floors, below).
  - Isometric y-sorted layers sort by diagonal bands (cells of equal x + y), which share a screen y. Staggered and hex layers sort by grid row.
  - Tall tiles sort at their base.
  - A reserved tile property, `sort_offset` (pixels), moves a tile's sort point for art whose base is not its cell's bottom. Godot's per-tile Y Sort Origin is the survey's precedent.
  - The draw list gains this ground key beside z and screen y. It is crate-internal, so no game code changes.
- **Plain layers draw in Tiled's exact order, even across tilesets.** On isometric, staggered and hex maps, tiles taller or wider than their cell overlap their neighbours. Tiled draws a layer row by row whatever tileset a tile comes from (on isometric maps by grid row, as the research measured).
  - 6a draws each chunk one tileset at a time, which differs from Tiled wherever two tilesets' tiles overlap, or a wide tile crosses a chunk border.
  - A layer with no tile that spills out of its cell keeps 6a's fast path, since nothing overlaps.
  - A layer with spilling tiles draws in painter's order across tilesets.
  - The speed pages use tall art, and any miss of the frame target is measured and reported as 6a's were.
- **Floors: an entity stands on the ground, and its floor lifts its drawing.**
  - An entity's `Transform` is where it stands in the map's ground space. Its sprite draws lifted by its floor's height. So physics, sorting and the floor's colliders share one space, and sorting never jumps when a character changes floor.
  - A layer's reserved `floor` property (an integer, default 0) puts it on a floor. A floor's height is how far Tiled draws its layers above the ground: their vertical offset, as designers already shift upper storeys in Tiled.
  - The floor's tiles draw lifted, as Tiled shows them. Its colliders, occluders, objects and class tiles sit in ground space, with the lift taken off.
  - The cell under the mouse checks floors from the top down, through each floor's lift.
  - **Walkable area:** on floor 0, collision is as in 6a. On a higher floor, the cells its layers fill are walkable, and the edge of that footprint collides, so a character cannot walk off a raised floor into the air. Stairs open that edge.
  - **Stairs and ramps** are tiles on the lower floor's layer with a reserved `stairs` property naming the floor they lead to. A character crossing stair cells has its drawn lift eased from one floor's height to the other's. It changes floor where it leaves the stairs onto the other floor's cells. Walking back down is the same crossing reversed. The builder settles how a game hooks in.
  - **Sorting:** on a map with floors, all its y-sorted layers sort together with the characters at one z, so floors interleave correctly. Maps without floors keep 6a's rule (each y-sorted layer at its own z). Plain layers keep 6a's z order.
- **Collision by floor, without taking games' collision bits.**
  - Colliders on different floors never interact, between tiles and entities and between entities. This is a floor filter in physics (Rapier's contact and intersection filters, and the character sweep's query filter), separate from the 32 `layer`/`mask` bits games own.
  - An entity without `Floor` is on floor 0. A game can put an entity on every floor, for a trigger that should catch all of them.
- **Shadows and floors.** A light carries a floor like an entity. A light on a floor is blocked only by that floor's occluders, and a light set to every floor sees all of them.
- **Hex rotations in the cell.** Tiled's hexagonal flags (`0x20000000` rotates 60° clockwise and `0x10000000` rotates 120° on hex maps; flips apply first) become a fourth bit in the tile's flips. The cell has room for it (bit 31 is free). The research measured the order, and the builder confirms it against Tiled's renders.
- **One sparse grid for every map.**
  - A tile layer stores its cells in chunks by signed chunk coordinate, with 6a's per-chunk versions. A bounded map is a sparse grid with bounds.
  - Collision, occluders, the chunk renderer and edits read chunk versions as they do now, with signed coordinates.
  - Outline points become relative to their chunk. That removes 6a's 8,192-cells-a-side collision cap (`docs/backlog/tilemap.md`), which 6a's notes proposed as the route.
- **Streaming design.**
  - **The generator:** a function a game gives a map, on a component of its `MapRoot`. Given a chunk's coordinates, it returns the chunk's cells for each layer. It must be `Send + Sync` and runs on its own builder thread, not the map-landing thread, so a chunk never waits behind a whole map.
  - **The web has no threads.** There, chunks are generated on the main thread within a per-frame time budget, and the rest wait for the next frame. The skill says so.
  - **Anchors:** cameras, plus any entity with a stream-anchor component.
  - **Load and unload:** chunks within the load radius of any anchor load. A chunk unloads once it is past a larger unload radius from every anchor. The gap between the two radii stops chunks thrashing at the edge.
  - **Saving:** a chunk changed since it was generated or loaded is handed to the game as a message carrying its cells before it unloads. The game can answer later chunk requests from its saved store instead of the generator.
  - **Entities:** class tiles in a streamed chunk spawn as entities and unload with it. The engine sends a message when a chunk loads, so a game can place its own entities there.
  - **Memory:** resident chunks are bounded by the radii.
- **Tiled infinite maps.**
  - A Tiled infinite map lands whole, like a bounded map. Its bounds are its chunks' bounds, and its origin is cell (0, 0), wherever the chunks lie.
  - **With a generator attached:** any engine chunk that any layer's Tiled chunk touches belongs wholly to the file. The generator never runs there, and the parts Tiled left empty stay empty. So a deliberately empty plaza next to hand-made ground is not filled in. The generator fills only chunks the designer never touched.
  - The file's chunks stay resident, because they are the asset's.
  - Hot reload diffs the chunk sets. A save that changes a chunk replaces it, and chunks the generator made survive a reload.
- **Propagation design: walk up from what changed.** The fix collects the entities whose transform or parent changed and updates each from its parent's stored global transform. It descends only where a result changed. This needs no marker components, which would churn every frame across propagation's three runs a frame.
  - The per-entity change scan remains. The research expects most of 6a's miss gone, not all.
  - If the target is still missed, the builder measures keeping static map parts (collider parts and occluder chains) out of the scanned set, and reports.
- **Navigation topology.** The navigation grid gains a topology: square, isometric (square in cell space, with step costs from world distance so paths look shortest on screen) or hexagonal.
  - Neighbours, the distance heuristic, world conversion and line of sight follow it.
  - `block_tiles` takes the map's geometry.
  - For infinite maps the grid covers a window the game chooses, such as the loaded region around its anchors.
  - **Across floors:** each floor is a layer of the graph, and the stair cells link the floors they join, so a path can climb from one floor to another. A path's steps carry their floor, so a character following it changes floor as the stairs do.
- **Worlds.** A `.world` file places a map by its tile origin. Bounds come from the grid geometry for every orientation, and from the chunks' bounds for an infinite map. Tiled's handling of infinite and isometric maps in worlds is undocumented; the builder verifies it with Tiled if it can, else takes this reading and says so in the skill.
- **Details the builder verifies in Tiled before building on them,** and records in the research notes:
  - whether a tile's collision shapes are stored in isometric coordinates when its tileset's grid is isometric (the design assumes image pixels; Bevy's Tiled crate was caught by this);
  - whether collision shapes on oblique maps are skewed;
  - the hex rotation and flip order.
- **Terrain painting on staggered maps** follows what Tiled's own terrain code paints there. Fixtures are painted by Tiled's scripting (`wangEdit`), as 6a did for orthogonal maps; staggered neighbours depend on row parity. Isometric terrain keeps 6a's rules, since its cells form a square lattice.
- **The demo is a new one, with Kenney's CC0 art.** It is a second tile demo beside 6a's eleven-page one, so neither grows too long. The art is Kenney's isometric and hexagon packs (or another CC0 source), with every pack's licence checked and recorded in the demo's asset README, as 6a did. Its pages:
  - an isometric town with floors and stairs;
  - a staggered map;
  - a hex strategy board (picking, neighbours, range, line, and pathfinding);
  - an oblique map;
  - a Tiled infinite map;
  - an endless streamed world, where edits survive leaving and coming back;
  - a speed page per grid.

### Floors: details settled at planning (2026-11-06, my calls)

Mapping the code for floors found what the calls above leave open. These settle it; the owner may revisit them.
- **A floor's height and ground space.**
  - A floor's height is the vertical part of the offset of its bottom layer (the first in draw order). The ground floor's height is always zero, so its layers keep their whole offset as before.
  - A layer on a higher floor stands at its offset less its floor's height. Its objects, class tiles, colliders and shadow chains are therefore in ground space without further work, and the drawer adds the height back.
  - A group layer's `floor` passes to its children, because designers group a storey's layers.
- **Values.**
  - `floor` (a layer's) and `stairs` (a tile's) are integers from 0 to 255. Any other value refuses the map with an error naming the layer or tile.
  - Stairs lead up from their layer's floor. Stairs that name a floor the map lacks, their own floor or a lower one are warned once and stay plain tiles.
  - LDtk layers have no properties, so LDtk maps have no floors. LDtk's world depths are not floors.
- **What draws lifted.**
  - An entity with `Floor` draws lifted by that floor's height on the map under it, and so does every entity below it in the hierarchy (a sprite on a child, a held torch).
  - That covers sprites and their shadows, composites, world text, particles (at emission), trails (when recorded), and lights with their shadows.
  - Lines and the physics overlay stay in ground space. A camera that follows a character adds the character's lift (`MapsView::drawn_lift`).
- **Sorting.**
  - On a map with floors, every y-sorted layer draws at the z of the map's first y-sorted layer (`MapsView::y_sort_z`).
  - Rows and entities sort by their ground y. At an equal y the lower floor draws first.
- **Stairs.**
  - An entity opts in with `Climber`.
  - A run of stair cells leads from its layer's floor to the floor its `stairs` names. Its top is the sides it shares with the upper floor's cells, and its bottom is the sides it shares with cells outside both the run and the upper floor.
    - *Ruled in building (2026-11-07):* the bottom is the run's foot, the farthest of those outside sides from its top along the run, so the lift eases over the run's whole length (`stairs.rs`, `ends`; tilemap module notes).
  - A climber's drawn height eases between the two floors' heights by its progress from bottom to top. The progress is worked out each frame from its drawn position, so it blends like movement.
  - **A climber changes floor halfway along the run, not where it leaves the stairs** as the call above says. The upper floor's edge encloses its stairs, and the lower floor may have walls where the stairs arrive. Switching at the middle lets a character leave the stairs at both ends without opening either.
  - A `FloorChanged` message names each change.
- **Collision.**
  - A higher floor's walkable footprint is the cells its layers fill plus the stairs that lead to it. The footprint is traced as a grid of its own on a `MapFloor` entity, and its hollow outline is the edge.
  - On the ground floor, a raised area blocks only where the designer gives it collision, so a bridge can stand over open ground.
- **Physics.**
  - The floor filter is a Rapier hook reading the floor from each collider's spare user-data bits. It is set only on colliders off the ground floor and not on every floor, so ground-floor pairs keep Rapier's contact reuse.
  - A collider's floor is its own entity's `Floor`, else its body's.
  - Game queries see every floor unless filtered (`QueryFilter::on_floor`).
  - A respawn takes the floor of the checkpoint it last touched.
- **Lights.**
  - A light on a floor is blocked by that floor's occluders and by occluders on every floor. The light and its shadows draw lifted.
  - A light still brightens whatever lies under it on screen, whatever its floor: there is one light image per camera, and that is a recorded limit.
  - A light on every floor draws at ground height.
- **Paths.**
  - `MapsView::navigation` gives one floor's grid.
  - `MapsView::floor_navigation` gives every floor's grid, linked at each stair cell between the floors the stairs join, at no cost, since the cell is the same ground.
    - *Ruled in building (2026-11-07):* a link costs a tie-breaking 1e-3 of the grid's shortest step (`LINK_COST`), so a path crosses a run of stairs once (navigation module notes).
  - Steps carry their floor (`PathNode::floor`). Smoothing never skips across a link.

## Stages

6b merges in six stages (the owner's decision; the order and the cuts are my call). Each stage has its own plan, review and hand-play. Each merges with main working, its docs, module notes and skills current, and the new demo grown by its pages. A feature a later stage brings stays refused with a clear error until then.
1. **Transform propagation.** Propagation skips unchanged subtrees, and 6a's static-map target is met. It is engine-wide and stands alone.
2. **Sparse grids and infinite maps.**
   - Signed, chunked grids everywhere: tile layers, collision, occluders, the renderer and edits.
   - Chunk-relative outline points, removing the 8,192-cells-a-side collision cap.
   - Tiled's infinite maps (orthogonal), and maps grown at run time.
3. **Streaming.** Generators, anchors, load and unload radii, the save hook and the web's budget, on orthogonal maps. This includes streaming a Tiled infinite map with a generator.
4. **Grid shapes.**
   - The grid geometry; isometric, staggered, hexagonal and oblique maps loading, drawing in Tiled's order, placing objects, editing and sorting with characters.
   - Hex rotations, the grid tools, terrain painting on isometric and staggered maps, and worlds.
   - Collision and shadows on the square-lattice grids (isometric, staggered, oblique).
   - Infinite maps and streaming work on these grids.
   - Hex layers that collide or occlude stay refused until stage 5.
5. **Hex collision and pathfinding.** The hex outline tracer, hex collision and shadows, and the navigation topology for every grid, infinite windows included.
6. **Floors.** Floor layers, lifted drawing, sorting across floors, the floor filter in physics, walkable footprints, stairs, lights per floor, and paths across floors.

## Design

### Assets

- **`TileMap`** gains its grid, read from Tiled's `orientation`, `staggeraxis`, `staggerindex`, `hexsidelength`, `skewx` and `skewy`. A missing skew is 0, and negative skews are legal. The map's `width` and `height` are meaningless for an infinite map (Tiled writes 0); its layers hold chunks.
- **Tile layers** read chunks in every encoding 6a reads, plus TMX `<chunk>` elements. Each chunk's size comes from the chunk itself, not from an assumed 16. A layer's bounds are derived from its chunks; JSON's `startx`, `starty`, `width` and `height` are a hint.
- **Tiled ignores `renderorder` on non-orthogonal maps.** They always draw right-down: rows top to bottom. On column-staggered and column-hex maps, shifted columns draw over their neighbours, which is painter's order by the cell's screen y. The engine matches.
- **Objects.**
  - On isometric maps, Tiled stores positions with both axes in tile-height units, along the diamond's axes. They convert to layer pixels on load. Rectangles and ellipses become polygons, since a rectangle on an isometric map is a diamond.
  - Oblique maps store unskewed pixels.
  - Staggered and hexagonal maps store plain pixels.
  - Tile objects anchor at the bottom centre on isometric maps and at the bottom left elsewhere, unless the tileset sets `objectalignment`.
- **6a's cell cap counts decoded chunk cells.** A map grown at run time is bounded by its streaming radii instead.
- **Refusals stay for what 6b does not do,** named in the error: a hex Wang set used for painting, and anything Tiled adds beyond 1.12.

### At runtime

- **Edits:** `ctx.maps()` edits work on every grid. On an infinite layer, writing past the current chunks creates them. Clearing a chunk's last tile leaves it in place; a streaming unload removes it.
- **Grid tools:** conversions, neighbours, distance, and hex lines, ranges and rings, exposed for each grid. The builder settles the names, shaped like 6a's `MapsView`.
- **Class tiles and objects** spawn at their cell's anchor point through the geometry.
- **Code-built maps:** `TileMap` built in code takes a grid. Procgen's conversions take a grid too, for square, isometric and staggered maps.

### Drawing

- **Placement:** each instance is placed through the geometry, with the tile image's bottom left on the cell box's bottom left plus the tileset offset, as Tiled measured on every grid.
  - Oblique tiles move with the skew but are not sheared.
  - Hex rotations rotate the quad about the cell box's centre.
  - The tile shader needs no new placement maths: the instance carries any affine transform.
- **Culling:** chunks cull by the bounding box of their cells through the geometry, tested against each camera before a chunk is built.
- **Pixel art:** the pixel-art snap keeps tiles crisp where half-cell offsets land on half pixels (staggered and hex rows, odd tile sizes). The builder pins it with a test per grid.

### Collision, shadows and navigation

- **Collision:**
  - Outlines are traced per grid lattice, with points relative to chunks, and mapped to layer pixels before colliders are built.
  - An edit retraces only the loops through its cells, as in 6a, on every grid.
  - Colliders stay one packed part per chunk.
- **The character controller's floor and wall rules** are checked on isometric diamonds and hex outlines: a sloped wall on a diamond must not count as a floor in a top-down game. The builder confirms the controller's top-down mode on these outlines.
- **Shadows** follow the same outlines, each floor's occluders apart.
- **Navigation** builds from a map's collision through the geometry, as described above.

### Streaming

- A streamed map is a `MapRoot` with a generator component. Its tilesets and grid come from a `TileMap`, from a file or built in code.
- Each frame a streaming system:
  - finds the chunks within the anchors' load radius that are not resident and asks for them (the game's store first, then the generator);
  - lands finished chunks within a per-frame budget;
  - unloads chunks past the unload radius, handing changed ones to the game.
- A landed chunk marks its version, so drawing, collision and shadows pick it up through 6a's incremental paths.
- **An unloading chunk:**
  - its cells leave collision as empty, so loops through it close at the boundary;
  - its occluders and class tiles despawn;
  - the chunk renderer frees it.

### Transform propagation

- The walk-up design described above, in all three of propagation's runs each frame. It must keep everything the current tests pin: spawns placed at once, detaching, grouping entities, interpolation markers.
- A new bench moves one leaf of a big tree. 6a's `static_frame` and `edit` benches measure the result.

### Hot reload

- A saved Tiled map of any grid reloads as in 6a.
- Infinite maps diff by chunk. Generated chunks survive.
- Objects reconcile by id.

### The web

- Every grid and infinite file loads from the embedded files.
- Streaming generates chunks within a per-frame budget on the main thread.

### Headless and tests

- **Loader fixtures** are saved by Tiled 1.12.2 itself: every grid in both formats, infinite maps in every encoding, and hex rotations. The research's samples are a start.
- **GPU tests** pin, on every grid:
  - tile placement against Tiled's renders, and the draw order of overlapping tall tiles;
  - hex rotations and flips;
  - y-sorting with an entity on each side of a tall tile, and across two floors;
  - culling;
  - no seams.
- **Collision tests** pin:
  - hex and diamond outlines;
  - an edit retracing only its loops;
  - the floor filter between tiles and entities;
  - a raised floor's edge;
  - stairs changing floor both ways.
- **Pathfinding tests** pin hex and isometric paths, and a path climbing stairs to another floor.
- **Streaming tests** pin:
  - chunks loading and unloading as an anchor moves;
  - a changed chunk saved and restored;
  - no frame over budget while streaming;
  - memory bounded on a long walk.
- **Propagation tests** keep every current test and add the skipped-subtree cases.

## Public API and migration

- `TileMap` gains its grid, defaulting to orthogonal, so 6a's code builds unchanged.
- `TileColliders` and the chunk coordinates in public types (`MapOccluder::chunk`) become signed. Breaking changes are fine (the owner's standing rule).
- The prelude exports the grid types, the grid tools, `Floor`, the stream anchor and the generator component.
- The skills that mention tile maps, navigation or propagation are updated: `ryo-tilemap`, `ryo-api`, `ryo-physics`, `ryo-assets`, `ryo-procgen` and `ryo-ecs-internals`. Their Rust examples compile.

## Acceptance

### Correctness

- The demo's pages each show their goal with real art. Its isometric page shows a character walking in front of and behind tall walls and trees, and up stairs onto a floor.
- Every 6a test and demo still passes.
- Each refusal 6a pinned for 6b's features becomes a test that the feature loads and matches Tiled.

### Targets (my calls; measured and guarded like 6a's)

- **Static map:** a static 1024 x 1024 map of four layers adds at most 50 µs of CPU a frame over an empty frame on every grid, with a 1080p view of 16 px tiles. This is 6a's target, which the propagation fix must now meet for orthogonal maps too.
- **Edit:** editing one tile rebuilds only its chunk and the loops through it, within 100 µs for loops spanning up to four chunks, on every grid.
- **Load:** a 256 x 256 four-layer map of any grid, infinite or not, loads off the main thread within 20 ms.
- **Streaming:** an anchor crossing an endless generated world at 2,000 px a second with a 1080p view at zoom 1:
  - adds at most 0.5 ms of main-thread time to any frame on native platforms;
  - never shows an unfilled chunk on screen;
  - keeps resident chunks within the radii.

## Documentation

- The module notes are rewritten where 6b changes them: `src/core/tilemap/AGENTS.md`, `src/core/physics/AGENTS.md`, `src/core/render/AGENTS.md`, `src/core/ecs/AGENTS.md` (propagation) and the navigation module's notes.
- `docs/backlog/tilemap.md` records 6b's measured targets and limits.
- `docs/backlog/tilemap-features.md` is reduced to what stays later, with hex terrain added.
- The four research notes are committed under `docs/research/`.

## Risks

- **Sorting across floors** changes the draw list's ordering rules, which 6a's y-sorted layers and every entity rely on; the ordering tests guard it.
- **The floor filter** changes physics' contact filtering and the character sweep, which every game's collision passes through; the physics suites guard it.
- **Exact draw order across tilesets** may cost the frame target on layers with tall art, as 6a's y-sorted layers did; it is measured and reported.
- **The hex outline tracer** is the largest new piece. 6a's loop-local retracing, splitting and joining must carry over to the hex lattice.
- **Propagation is engine-wide.** A subtle change breaks interpolation, physics write-back or UI placement. Every current propagation test, and the physics and interpolation suites, guard it.
- **Streaming meets every subsystem at once:** drawing, collision, shadows, navigation, class tiles, reload. Its tests run them together.
- **Tiled parity on details the docs underplay:** hex rotation order, object anchors and infinite maps in worlds. Fixtures saved and measured from Tiled remove the guesswork, as in 6a.

## Out of scope

- Terrain painting on hexagonal maps, and hexagonal procgen generators.
- Automatic loading of a world's levels.
- The first-landing frame miss and the other limits 6a recorded, except the propagation fix and the collision side cap, which 6b takes.
- Per-tile run-time overrides of tileset data, saving maps back to the editors, and an in-engine tile editor.
