# AGENTS.md - Tilemap Module

## Status / Gotchas

- **Rendering is a data stub**: nothing in the renderer consumes `TilemapManager` yet. GPU chunk rendering (entries in the frame's `DrawList`) is planned.
- **Only orthogonal is implemented**: `Tilemap::orientation` stores `TilemapOrientation` (Isometric, Hexagonal*), but coordinate conversion ignores it.

## Design Decisions

### Why Dense Grid (Not ECS Per-Tile)?

Static tiles in a flat `Vec` give O(1) lookup and cache-friendly iteration. Promoting tiles to entities would add ECS overhead for data that rarely changes. The hybrid model reserves entities for chunks (rendering units) and behavioral tiles (animated, interactive).

### Why 16x16 Chunks?

16x16 balances draw call count against rebuild cost. A 200x200 map = ~169 chunks per layer, with typically 50-100 visible. Each chunk is one GPU draw call when rendering is wired up.

### Why Greedy Merge for Collision?

Reduces collider count dramatically (100 ground tiles → ~5 rectangles), which directly improves physics performance. The algorithm is simple, fast (single pass), and produces good results for platformer-style layouts.

### Why JSON-Only Tiled Import?

Tiled's JSON format (.tmj/.tsj) is simpler to parse than TMX (XML) and serde_json is already a dependency. The JSON format contains all the same data as TMX.
