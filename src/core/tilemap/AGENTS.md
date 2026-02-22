# AGENTS.md - Tilemap Module

## Overview

Grid-based tilemap system for 2D worlds. Provides tile storage, chunked viewport culling, greedy-merge collision generation, Tiled JSON import (.tmj/.tsj), and bitmask auto-tiling. Integrates with the engine via `TilemapManager` on `SceneContext`.

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Public exports and module documentation |
| `tileset.rs` | `Tileset`, `TileId`, `TileProperties`, `TileAnimation`, UV calculation |
| `layer.rs` | `TilemapLayer` — dense flat grid of `Option<TileId>` |
| `tilemap.rs` | `Tilemap`, `TilemapOrientation`, coordinate conversion, tile queries |
| `chunk.rs` | `CHUNK_SIZE`, `ChunkCoord`, `ChunkState`, viewport culling math |
| `collision.rs` | `CollisionRect`, greedy rect-merge collision generation |
| `manager.rs` | `TilemapManager` — entity-keyed tilemap registry with dirty tracking |
| `tiled.rs` | Tiled JSON loader, `TiledObject`, `TiledShape`, `TiledValue`, `TilemapError` |
| `autotile.rs` | `AutoTileSet`, `AutoTileMode`, bitmask computation (4-bit minimal, 8-bit full) |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Game Code                                │
│  - Build Tilemap programmatically or load via TiledLoader    │
│  - Register with ctx.tilemap (TilemapManager)                │
│  - Query tiles, coordinates, solidity                        │
│  - Generate collision rects for physics                      │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                  TilemapManager                              │
│  - Tracks registered tilemaps by Entity                      │
│  - Maintains per-chunk dirty state                           │
│  - Provides tile get/set, coordinate conversion              │
│  - Viewport culling via visible_chunks()                     │
└─────────────────────────────────────────────────────────────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
┌──────────────────┐ ┌──────────┐ ┌──────────────┐
│    Tilemap       │ │  Chunks  │ │  Collision   │
│ Layers, Tilesets │ │ 16x16    │ │ Greedy merge │
│ Tile queries     │ │ Dirty    │ │ Rect output  │
└──────────────────┘ └──────────┘ └──────────────┘
```

## Key Concepts

### Data Model

**Tileset** — A sprite atlas divided into a uniform grid of tiles, referenced by `TileId` (u32). Each tile can have `TileProperties` (solid flag, animation, custom key-value pairs). UV coordinates for any tile are computed via `tile_uv()` accounting for spacing and margin.

**TilemapLayer** — A single grid layer stored as a flat `Vec<Option<TileId>>` indexed as `tiles[y * width + x]`. Each layer references one tileset by index.

**Tilemap** — Top-level container holding tilesets, layers, and map dimensions. Provides coordinate conversion (`world_to_tile`, `tile_to_world`) and tile queries (`tile_at_world`, `is_solid_at`). Supports `TilemapOrientation` (Orthogonal, Isometric, HexagonalFlat, HexagonalPointy) — only orthogonal is implemented in phase 1.

### Chunk System

Tiles are grouped into 16x16 chunks for rendering and dirty tracking.

- `ChunkCoord` identifies a chunk by (chunk_x, chunk_y, layer_index)
- `ChunkState` tracks `dirty` (needs rebuild) and `has_tiles` (skip if empty)
- `chunk_visible()` performs AABB frustum test against a viewport rect
- `chunks_along_axis()` and `chunk_tile_range()` handle subdivision math

When a tile is modified via `TilemapManager::set_tile()`, the containing chunk is marked dirty.

### Collision Generation

`generate_collision_rects()` uses a greedy rect-merge algorithm to combine adjacent solid tiles into minimal rectangles:

1. Scan tiles left-to-right, top-to-bottom
2. For each unvisited solid tile, expand right as far as possible
3. Then expand down while the full row width remains solid
4. Output a `CollisionRect` and mark all covered tiles visited

A 100-tile ground plane produces ~1-5 rectangles instead of 100 individual colliders.

`generate_collision_rects_region()` does the same for a sub-region (useful for per-chunk regeneration).

### Tiled Import

`load_tiled_map(path)` parses Tiled JSON files (.tmj) and returns `(Tilemap, Vec<TiledObject>)`:

- Resolves external tilesets (.tsj) relative to the map file
- Converts GID-based tile references to local tile IDs per tileset
- Parses tile properties (solid, custom) from tileset data
- Extracts object layers as `TiledObject` with shape, position, and properties
- Supports `TiledShape` variants: Rect, Ellipse, Point, Polygon, Polyline
- Properties mapped to `TiledValue`: Bool, Int, Float, String, Color

### Auto-Tiling

Bitmask-based auto-tile selection. Two modes:

| Mode | Bits | Variants | Description |
|------|------|----------|-------------|
| `Minimal` | 4 (cardinal) | 16 | Checks N/E/S/W neighbors only |
| `Full` | 8 (cardinal+diagonal) | 47 | Checks all 8 neighbors with diagonal reduction |

**Diagonal reduction**: A diagonal neighbor only counts if both adjacent cardinal neighbors are also present. This reduces 256 possible bitmasks to 47 meaningful tile shapes.

`AutoTileSet` stores rules as a `HashMap<u8, TileId>` mapping bitmask → tile ID, with a `default_tile` fallback.

## Usage Patterns

### Programmatic Creation

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
    let tileset = Tileset { /* ... */ };
    let mut tilemap = Tilemap::new(32, 32, 20, 15);
    tilemap.add_tileset(tileset);

    let mut layer = TilemapLayer::new("ground", 20, 15);
    layer.set_tile(5, 10, 0);  // Place tile ID 0 at (5, 10)
    tilemap.add_layer(layer);

    let entity = ctx.world.spawn(());
    ctx.tilemap.register(entity, tilemap);
}
```

### Loading from Tiled

```rust
let (tilemap, objects) = load_tiled_map("assets/maps/level1.tmj")?;
let entity = ctx.world.spawn(());
ctx.tilemap.register(entity, tilemap);

let spawns: Vec<_> = objects.iter()
    .filter(|o| o.obj_type == "spawn_point")
    .collect();
```

### Runtime Tile Editing

```rust
ctx.tilemap.set_tile(map_entity, 0, x, y, Some(tile_id));  // Place
ctx.tilemap.set_tile(map_entity, 0, x, y, None);            // Clear
// Affected chunk is auto-marked dirty
```

### Collision Generation

```rust
let rects = generate_collision_rects(ctx.tilemap.get_tilemap(entity).unwrap());
for rect in &rects {
    let (wx, wy, ww, wh) = rect.to_world(32, 32);
    // Spawn collider entity at (wx, wy) with size (ww, wh)
}
```

### Coordinate Conversion

```rust
let tile_coords = ctx.tilemap.world_to_tile(entity, world_x, world_y);
let (wx, wy) = ctx.tilemap.tile_to_world(entity, tile_x, tile_y).unwrap();
let is_wall = ctx.tilemap.is_solid(entity, world_x, world_y);
```

## Integration Points

- **SceneContext**: `ctx.tilemap` provides the `TilemapManager` instance
- **Engine**: Creates and owns `TilemapManager`, passes it to scene callbacks
- **Prelude**: All public types re-exported via `ryo2d::prelude::*`
- **Rendering**: Currently a data stub — GPU chunk rendering via `BatchCollector`/`SpriteBatch` is planned for a future phase

## Design Decisions

### Why Dense Grid (Not ECS Per-Tile)?

Static tiles in a flat `Vec` give O(1) lookup and cache-friendly iteration. Promoting tiles to entities would add ECS overhead for data that rarely changes. The hybrid model reserves entities for chunks (rendering units) and behavioral tiles (animated, interactive).

### Why 16x16 Chunks?

16x16 balances draw call count against rebuild cost. A 200x200 map = ~169 chunks per layer, with typically 50-100 visible. Each chunk is one GPU draw call when rendering is wired up.

### Why Greedy Merge for Collision?

Reduces collider count dramatically (100 ground tiles → ~5 rectangles), which directly improves physics performance. The algorithm is simple, fast (single pass), and produces good results for platformer-style layouts.

### Why JSON-Only Tiled Import?

Tiled's JSON format (.tmj/.tsj) is simpler to parse than TMX (XML) and serde_json is already a dependency. The JSON format contains all the same data as TMX.
