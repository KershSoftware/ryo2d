# Tile Maps: What Stays for Later

Tile maps load from Tiled and LDtk on every grid Tiled has (orthogonal, isometric, staggered, hexagonal and oblique), from files and built in code, bounded or infinite, and stream endless worlds round the cameras and anchors: drawing, objects, picking and the board tools, edits and Wang terrain, collision and shadows, y-sorting, streaming, worlds and hot reload follow each map's grid. Designs: `docs/specs/2026-10-29-tilemap-design.md` and `docs/specs/2026-10-31-tilemap-shapes-design.md`; using them from a game: the `ryo-tilemap` skill; what they cost, and their limits: `docs/backlog/tilemap.md`; a demo of each grid: `examples/demos/tile-grids`. This is what the designs left for later. Each is refused with a clear error, or warned once where a file is otherwise usable, until it is done.

## Hexagonal collision and shadows

A hexagonal map's tile layers neither collide nor cast shadows: one whose `collides` or `occludes` is true fails to load, naming the layer, and a hexagonal layer made to collide in code builds nothing, warned once a layer.

- A hexagonal cell has six neighbours, so `TileColliders`' square outline trace (units one or two cells decide, `physics/tiles/outline.rs`), which isometric, staggered and oblique maps share through their square lattice (`MapGrid::lattice`), does not apply, and a hexagonal grid has no lattice.
- It needs a hex grid kind that traces the boundary of hexagons the same way (shared edges cancel, loops stay seamless, an edit retraces only the loops through it, packing by chunk), or per-cell polygons merged across shared edges, with the character controller's floor and wall rules checked on hex outlines. Shadows would follow the same outlines. It is the largest piece left, and it is physics'.

## Navigation off the orthogonal grid

`NavigationGrid::block_tiles` reads a square grid of upright rectangles. Over an isometric, staggered or hexagonal map, or an oblique one that is skewed, it blocks nothing, and the dev log warns once a navigation grid.

- Isometric, staggered and oblique maps could block by cell in their square lattice, with step costs from world distance.
- Hexagonal maps need a hex graph (six neighbours) or a sampled square grid.
- The design asks for paths over the loaded part of an infinite map, and across floors (below), on one graph linked at stairs.

## Floors

A layer belonging to a floor (raised ground, an upper storey, a bridge), on every grid:
- characters draw in front of and behind that floor's tiles and the floors round it, every y-sorted layer of a map with floors sorting with the characters at one z;
- an entity stands on the ground and its floor lifts its drawing; the cell under the mouse checks floors from the top down, through each floor's lift;
- colliders on different floors never interact (a floor filter in physics, apart from the 32 layer and mask bits games own), and a light on a floor is blocked only by that floor's occluders;
- characters change floors through stairs and ramps the designer marks.

Until then, raised ground a character stands on is drawn as the tile-grids demo's town draws it: blocks drawn a cell lower than their props, their tops on the cells, with one floor.

## Terrain painting on hexagonal maps

`set_terrain` on a hexagonal map warns once, paints nothing and returns false. A hexagon has six neighbours where a Wang id has four corners and four edges, and Tiled's own terrain brush does not support hexagonal maps (its issue #2824), so there is no editor to author hex terrain sets for a run-time feature to read.

## Hexagonal generators in the procgen crate

Procgen's generators stay square. Their grids convert to square, isometric, staggered and oblique maps, which share the square lattice; a hexagonal map has none.

## Other later items

- Running Tiled's automapping and LDtk's auto-layer rules at run time: both run in their editors, which save the tiles.
- Per-tile run-time overrides of tileset data (Godot's run-time tile data).
- Saving maps back to the editors' formats, and a tile editor inside the engine.
- Tiled's text objects beyond their placement (`ObjectShape::Text` keeps the text; nothing draws it).
- Streaming a world's levels automatically: worlds give positions, neighbours and `spawn_levels_in`, and when to load and unload stays the game's call.
- LDtk level background images.
- What `docs/backlog/tilemap.md` lists as limits: the landing frame's first draw, relabelling long split loops, lighting's lack of a spatial index, the keep window's memory.

## Outside tile maps

The 2026-02 survey of Godot, Unity, GameMaker, Defold, Bevy, Love2D, Phaser and Pygame also noted gaps outside tile maps. Skeletal animation, joints and capsule, polygon, segment and compound colliders have been built since; these were still open then (check before working on one): localization (string tables, plural rules, locale switching), accessibility (screen readers, text to speech), sprite atlas packing at build time, normal-map lighting for 2D sprites, tweening along Bézier and spline paths, and texture compression at build time (DXT, ETC2, ASTC).
