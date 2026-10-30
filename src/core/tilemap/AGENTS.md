# AGENTS.md - Tilemap Module

Sub-project 6a replaces the old tilemap module. Design: `docs/specs/2026-10-29-tilemap-design.md`. What exists so far is the data model and the Tiled JSON loaders; rendering, collision and the rest arrive task by task.

## What exists

- `tile.rs`: `Tile` (tileset index, id, `TileFlip`) and `TileFlip` (H, V, D bits as Tiled stores them; `rotated_cw` uses Tiled's table).
- `property.rs`: `PropertyValue` and `Properties` (insertion-ordered, typed getters; `int` accepts a whole float, `float` accepts an int).
- `tileset.rs`: `Tileset` (an `Asset`; atlas or collection; per-tile `TileData` with shapes, animation, probability; `WangSet`s) and `URect`, the engine's own texel rectangle (glam has none).
- `grid.rs`: `TileGrid`, a dense layer of 4-byte cells (0 empty; flips in bits 28..=31, tileset in 20..=27, id + 1 in 0..=19) cut into `CHUNK_SIZE` (32) chunks. Edits mark chunks dirty; `drain_dirty` hands each changed chunk out once in row order. A new grid has no dirty chunk. A tile that does not fit a cell (tileset over 255, id over 1,048,574) is refused by `set`.
- `map.rs`: `TileMap` (an `Asset`) of `MapLayer`s (`LayerInfo` plus a `LayerKind` of tiles, objects or an image) over `TilesetSource`s (asset handle or embedded `Arc`). `IntGrid::get` returns `None` for 0 as well as outside.

## Gotchas

- `tiled/`: Tiled's formats as registry assets. `raw.rs` holds what a file says (serde structs named as Tiled's JSON names them; the XML reader fills the same ones, so every non-zero default is an `Option` and `build.rs` owns the defaults), `data.rs` decodes gids and layer data (CSV, base64, gzip, zlib, zstd via `ruzstd`, all bounded by the layer's cell count), `build.rs` turns raw into `TileMap` and `Tileset` and holds the checks, `json.rs` the `TiledMapLoader` (`.tmj`) and `TiledTilesetLoader` (`.tsj`).
- A path written in a file joins that file's folder; the registry normalises it and refuses one leaving the asset root. An inline tileset's paths resolve against the map's folder.
- External tilesets are dependencies and are not loaded while the map loads, so a gid is matched to a tileset by `firstgid` alone. The ids each tile layer uses are noted in `TileMap::used_ids` and measured against the tilesets in the map's `check`; the atlas image against the declared tiles in the tileset's `check`.
- Group layers flatten: offset adds, opacity and tint multiply, parallax multiplies, visibility ands, a non-normal blend passes down. Layer ids stay as written; a layer without one gets the next free id.
- Limits, each a load error naming the place: 1<<26 cells a layer or map, 256 tilesets a map, 1,048,575 tiles a tileset. Unsupported until 6b: isometric, staggered, hexagonal and oblique maps and infinite maps; layer blend modes other than normal, add, multiply and screen.
- Object layers load as empty `LayerKind::Objects` for now; the tileset's tile offset is not kept.
- `LayerInfo::occludes: None` means "as `collides`"; `lit` defaults to true.
