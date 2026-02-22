# Tilemap System Design

**Date:** 2026-02-21
**Status:** Approved
**Approach:** Layered Tilemap Architecture with chunked GPU rendering (Approach 1)

## Overview

A full tilemap system for Ryo2D: grid-based tile rendering, auto-tiling, collision generation, Tiled JSON import (.tmj/.tsj), animated tiles, and isometric/hexagonal support. Tilemaps render through the existing GPU sprite batch pipeline using per-chunk instancing.

## Architecture

**Hybrid ECS model:** Static tiles live in dense grid arrays (one `TilemapLayer` component per layer). Chunks are entities for rendering. Tiles that need individual behavior (animated, interactive) get promoted to standalone entities automatically.

## Data Model

### Tileset

A sprite atlas divided into a grid of tiles, referenced by index.

```rust
pub struct Tileset {
    pub texture: SpriteHandle,
    pub tile_width: u32,
    pub tile_height: u32,
    pub columns: u32,
    pub tile_count: u32,
    pub spacing: u32,
    pub margin: u32,
    pub properties: HashMap<u32, TileProperties>,
}

pub struct TileProperties {
    pub solid: bool,
    pub animation: Option<TileAnimation>,
    pub custom: HashMap<String, String>,
}
```

### TilemapLayer

A single grid layer holding tile indices into a tileset.

```rust
pub struct TilemapLayer {
    pub name: String,
    pub tiles: Vec<Option<TileId>>,  // Dense flat grid, None = empty
    pub width: u32,
    pub height: u32,
    pub tileset_index: usize,
    pub visible: bool,
    pub opacity: f32,
    pub z_order: i32,
    pub parallax_factor: Vec2,
}

pub type TileId = u32;
```

### Tilemap

Top-level component holding everything together.

```rust
pub struct Tilemap {
    pub tilesets: Vec<Tileset>,
    pub layers: Vec<TilemapLayer>,
    pub tile_width: u32,
    pub tile_height: u32,
    pub map_width: u32,
    pub map_height: u32,
    pub orientation: TilemapOrientation,
}

pub enum TilemapOrientation {
    Orthogonal,
    Isometric,
    HexagonalFlat,
    HexagonalPointy,
}
```

## Chunk System & Rendering

Chunks are the rendering unit. Each chunk covers a 16x16 tile region of a single layer and pre-builds a `SpriteBatch` for GPU rendering.

```rust
const CHUNK_SIZE: u32 = 16;

pub struct TileChunk {
    pub chunk_x: u32,
    pub chunk_y: u32,
    pub layer_index: usize,
    pub dirty: bool,
    pub sprite_batch: SpriteBatch,
}
```

**Flow:**
1. Tilemap spawned -> engine subdivides each layer into chunks
2. Each chunk is a child entity of the tilemap with `(Transform, TileChunk)`
3. Dirty chunks rebuild their `SpriteBatch` from the tile grid
4. Each frame, frustum-cull chunks against camera viewport
5. Visible chunks submit their batch to `BatchCollector`
6. Tile modifications mark affected chunks dirty

**Performance:** 200x200 map with 3 layers = ~120K tiles, but only ~50-100 chunks visible at any time, each a single draw call.

## Collision Generation

Solid tiles are merged into rectangular colliders using a greedy rect-merge algorithm.

```rust
pub struct TilemapCollider {
    pub entity: Entity,
    pub rect: Rect,
}

impl Tilemap {
    pub fn generate_colliders(&self, world: &mut World) -> Vec<TilemapCollider>;
    pub fn regenerate_chunk_colliders(
        &self, world: &mut World, chunk_x: u32, chunk_y: u32,
    ) -> Vec<TilemapCollider>;
}
```

- Greedy merge: expand right, then expand down while full row is solid
- 100-tile ground plane -> ~5-10 colliders instead of 100
- Collider entities spawned as children with `(Transform, Collider, RigidBody::kinematic())`
- One-way platforms via `platform: true` tile property
- Runtime edits: `regenerate_chunk_colliders()` despawns old, creates new merged colliders

## Tiled Import (.tmj/.tsj)

JSON-only loader for Tiled map editor exports.

```rust
pub struct TiledLoader;

impl TiledLoader {
    pub fn load(path: &str) -> Result<Tilemap, TilemapError>;
    pub fn load_with_base(path: &str, asset_base: &str) -> Result<Tilemap, TilemapError>;
}

pub enum TilemapError {
    IoError(std::io::Error),
    ParseError(String),
    MissingTileset(String),
    UnsupportedFeature(String),
}
```

**Imported:** Map dimensions, tile layers, tilesets (with atlas textures via AssetManager), tile properties, object layers, layer properties (visibility, opacity, parallax, z-order).

### Object Layers

Tiled object layers are exposed as raw data for game logic (spawn points, triggers, camera bounds).

```rust
pub struct TiledObject {
    pub name: String,
    pub obj_type: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub rotation: f32,
    pub shape: TiledShape,
    pub properties: HashMap<String, TiledValue>,
}

pub enum TiledShape {
    Rect,
    Ellipse,
    Point,
    Polygon(Vec<Vec2>),
    Polyline(Vec<Vec2>),
}

pub enum TiledValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Color(Color),
}
```

## Auto-Tiling

Bitmask-based auto-tile selection. Each tile checks neighbors, computes a bitmask, and looks up the correct variant.

```rust
pub struct AutoTileRule {
    pub terrain_id: u16,
    pub bitmask: u8,
    pub tile_id: TileId,
}

pub struct AutoTileSet {
    pub name: String,
    pub terrain_id: u16,
    pub mode: AutoTileMode,
    pub rules: Vec<AutoTileRule>,
    pub default_tile: TileId,
}

pub enum AutoTileMode {
    Minimal,   // 4-bit, 16 variants (cardinal only)
    Full,      // 8-bit, 47 variants (cardinal + diagonal, reduced)
}

impl Tilemap {
    pub fn set_terrain(&mut self, layer: usize, x: u32, y: u32, terrain_id: u16);
    pub fn clear_terrain(&mut self, layer: usize, x: u32, y: u32);
    pub fn add_auto_tileset(&mut self, auto_tileset: AutoTileSet);
    pub fn recalculate_auto_tiles(&mut self, layer: usize);
}
```

**Presets:** `AutoTileSet::blob_47()` (full 47-tile blob) and `AutoTileSet::minimal_16()` (16-tile cardinal).

**Tiled terrain import:** Terrain sets from .tsj are converted to `AutoTileSet` rules automatically.

## Public API & Scene Integration

### TilemapManager

Added to `SceneContext` alongside existing managers.

```rust
pub struct SceneContext<'a> {
    pub world: &'a mut World,
    pub input: &'a InputState,
    pub camera: &'a mut Camera,
    pub audio: Option<&'a mut AudioManager>,
    pub assets: &'a mut AssetManager,
    pub tilemap: &'a mut TilemapManager,  // NEW
    // ...
}
```

### Loading & Spawning

```rust
let tilemap = TiledLoader::load("assets/maps/level1.tmj")?;
let map_entity = ctx.tilemap.spawn(tilemap, &mut ctx.world);

let spawn_points: Vec<&TiledObject> = ctx.tilemap.objects(map_entity)
    .filter(|obj| obj.obj_type == "spawn_point")
    .collect();
```

### Runtime Tile Manipulation

```rust
ctx.tilemap.set_tile(map_entity, layer, x, y, Some(tile_id));
ctx.tilemap.get_tile(map_entity, layer, x, y); // -> Option<TileId>
ctx.tilemap.set_terrain(map_entity, layer, x, y, terrain_id);

let (tile_x, tile_y) = ctx.tilemap.world_to_tile(map_entity, world_x, world_y);
let (world_x, world_y) = ctx.tilemap.tile_to_world(map_entity, tile_x, tile_y);
```

### Tile Queries

```rust
ctx.tilemap.is_solid(map_entity, world_x, world_y); // -> bool
ctx.tilemap.tiles_in_rect(map_entity, layer, rect);  // -> Vec<(u32, u32, TileId)>
ctx.tilemap.raycast(map_entity, start, end);          // -> Option<TileHit>
```

### Animated Tiles

Tiles with `TileAnimation` cycle frames automatically. `TilemapManager` ticks animation timers and marks chunks dirty on frame changes.

## Implementation Phases

1. **Core:** Data model, chunks, rendering, collision, Tiled import
2. **Auto-tiling:** Bitmask rules, Tiled terrain set import, presets
3. **Iso/hex:** Isometric and hexagonal coordinate math and rendering

## Out of Scope

See `docs/plans/future-tilemap-features.md` for deferred features (infinite maps, image layers, Wang tiles) and other engine feature gaps identified during research.
