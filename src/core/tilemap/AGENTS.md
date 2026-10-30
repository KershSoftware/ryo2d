# AGENTS.md - Tilemap Module

Sub-project 6a replaces the old tilemap module. Design: `docs/specs/2026-10-29-tilemap-design.md`. What exists so far is the data model; loaders, rendering, collision and the rest arrive task by task.

## What exists

- `tile.rs`: `Tile` (tileset index, id, `TileFlip`) and `TileFlip` (H, V, D bits as Tiled stores them; `rotated_cw` uses Tiled's table).
- `property.rs`: `PropertyValue` and `Properties` (insertion-ordered, typed getters; `int` accepts a whole float, `float` accepts an int).
- `tileset.rs`: `Tileset` (an `Asset`; atlas or collection; per-tile `TileData` with shapes, animation, probability; `WangSet`s) and `URect`, the engine's own texel rectangle (glam has none).
- `grid.rs`: `TileGrid`, a dense layer of 4-byte cells (0 empty; flips in bits 28..=31, tileset in 20..=27, id + 1 in 0..=19) cut into `CHUNK_SIZE` (32) chunks. Edits mark chunks dirty; `drain_dirty` hands each changed chunk out once in row order. A new grid has no dirty chunk. A tile that does not fit a cell (tileset over 255, id over 1,048,574) is refused by `set`.
- `map.rs`: `TileMap` (an `Asset`) of `MapLayer`s (`LayerInfo` plus a `LayerKind` of tiles, objects or an image) over `TilesetSource`s (asset handle or embedded `Arc`). `IntGrid::get` returns `None` for 0 as well as outside.

## Gotchas

- Both `TileMap` and `Tileset` are registered with the asset registry and have no loaders yet.
- `LayerInfo::occludes: None` means "as `collides`"; `lit` defaults to true.
