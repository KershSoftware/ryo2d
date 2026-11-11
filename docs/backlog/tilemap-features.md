# Tile Maps: What Stays for Later

Tile maps load from Tiled and LDtk on every grid Tiled has (orthogonal, isometric, staggered, hexagonal and oblique), from files and built in code, bounded or infinite, and stream endless worlds round the cameras and anchors: drawing, objects, picking and the board tools, edits and Wang terrain, collision and shadows, navigation, y-sorting, streaming, worlds and hot reload follow each map's grid. A map's layers stand on storeys (raised ground, upper floors, bridges), each colliding, casting shadows and drawn lifted on its own, which characters climb between by stairs and ramps and paths cross. Designs: `docs/specs/2026-10-29-tilemap-design.md` and `docs/specs/2026-10-31-tilemap-shapes-design.md`; using them from a game: the `ryo-tilemap` skill; what they cost, and their limits: `docs/backlog/tilemap.md`; a demo of each grid: `examples/demos/tile-grids`. This is what the designs left for later. Each is refused with a clear error, or warned once where a file is otherwise usable, until it is done.

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
- What `docs/backlog/tilemap.md` lists as limits: the landing frame's first draw, relabelling long split loops, lighting's lack of a light image per storey, the keep window's memory.

## Outside tile maps

The 2026-02 survey of Godot, Unity, GameMaker, Defold, Bevy, Love2D, Phaser and Pygame also noted gaps outside tile maps. Skeletal animation, joints and capsule, polygon, segment and compound colliders have been built since; these were still open then (check before working on one): localization (string tables, plural rules, locale switching), accessibility (screen readers, text to speech), sprite atlas packing at build time, normal-map lighting for 2D sprites, tweening along Bézier and spline paths, and texture compression at build time (DXT, ETC2, ASTC).
