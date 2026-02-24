# AGENTS.md - Texture Management

This module provides centralized GPU texture management for Ryo2D. All content textures (not render targets) are loaded, stored, and accessed through a single `TextureManager` that lives on the engine. Renderers receive lightweight handles rather than raw GPU objects, enabling safe lifecycle management and deduplication.

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Public exports: `TextureHandle`, `TextureArrayHandle`, `TextureManager`, `TextureContext` |
| `handle.rs` | Generational `Handle<M>` type, `SlotMap<M, T>` allocator, marker types for type safety |
| `atlas.rs` | `slice_atlas()` utility for splitting raw RGBA image data into individual tile buffers |
| `manager.rs` | `TextureManager` (owns all GPU textures), `TextureContext` (ergonomic wrapper), `ManagedTexture`, `ManagedTextureArray` |

## Key Types

### `Handle<M>` (handle.rs)

Generic generational index with a phantom type parameter `M` to prevent mixing handles from different collections. Contains `index`, `generation`, and `PhantomData<M>`.

### `TextureHandle` / `TextureArrayHandle` (handle.rs)

Type aliases for `Handle<TextureMarker>` and `Handle<TextureArrayMarker>`. These are the public-facing handle types used by game code and renderers.

### `SlotMap<M, T>` (handle.rs)

Internal generational slot allocator. Supports `insert`, `get`, `get_mut`, `remove` with free-list reuse. When a slot is removed, its generation increments so stale handles return `None` on lookup.

### `ManagedTexture` / `ManagedTextureArray` (manager.rs)

Internal structs holding GPU resources and metadata:
- `ManagedTexture`: `wgpu::Texture`, `wgpu::TextureView`, `width`, `height`, `data` (CPU copy)
- `ManagedTextureArray`: `wgpu::Texture`, `wgpu::TextureView`, `width`, `height`, `layer_count`

### `TextureManager` (manager.rs)

Owns all content GPU textures. Provides:
- `load(path, device, queue)` - Load from disk with path-based deduplication
- `load_atlas(path, tile_w, tile_h, device, queue)` - Load and slice an atlas into individual tiles
- `from_rgba(data, width, height, device, queue)` - Create from raw RGBA bytes (procedural textures)
- `create_array(handles, device, queue)` - Build a `texture_2d_array` from individual texture handles
- `get_view(handle)` / `get_array_view(handle)` - Get `TextureView` references for bind group creation
- `dimensions(handle)` - Query texture dimensions
- `drop_texture(handle)` / `drop_array(handle)` - Explicitly free GPU memory

### `TextureContext<'a>` (manager.rs)

Convenience wrapper that bundles `&mut TextureManager` with `&wgpu::Device` and `&wgpu::Queue`. Obtained via `ctx.textures()` in scene callbacks. Mirrors the `TextureManager` API without requiring device/queue parameters:

```rust
let mut tex = ctx.textures();
let wall = tex.load("assets/wall.png");
let tiles = tex.load_atlas("assets/atlas.png", 64, 64);
let array = tex.create_array(&tiles);
```

## Architecture

### Ownership Chain

```
Engine
  └── TextureManager (owns all GPU textures and CPU data copies)
        ├── SlotMap<TextureMarker, ManagedTexture>      (individual textures)
        ├── SlotMap<TextureArrayMarker, ManagedTextureArray> (texture arrays)
        └── HashMap<PathBuf, TextureHandle>             (dedup cache)

SceneContext
  └── textures() -> TextureContext (borrows manager + device + queue)

Renderers (own their bind groups, reference TextureManager for views)
  ├── GpuRaycastRenderer: stores TextureArrayHandle, builds bind groups from manager views
  └── SpriteGpuRenderer: stores HashMap<TextureHandle, BindGroup> as bind_group_cache
```

Textures are created through `TextureManager`. Renderers receive handles and look up `TextureView` references from the manager when building bind groups. Renderers own and cache their own `wgpu::BindGroup` objects because bind group layout is renderer-specific.

### Data Flow

```
Game Code                        TextureManager              Renderer
    │                                  │                         │
    │  ctx.textures().load("wall.png") │                         │
    │ ──────────────────────────────> │ Decode image             │
    │                                  │ Upload to GPU            │
    │  <── TextureHandle ──────────── │ Store in SlotMap          │
    │                                  │                         │
    │  renderer.set_wall_textures(h)   │                         │
    │ ─────────────────────────────────────────────────────────> │
    │                                  │                         │
    │  (render time)                   │ get_array_view(h)       │
    │                                  │ <────────────────────── │
    │                                  │ ── &TextureView ──────> │
    │                                  │                         │ Build bind group
    │                                  │                         │ Draw
```

## Usage Patterns

### Loading from Disk

```rust
let mut tex = ctx.textures();
let handle = tex.load("assets/textures/brick.png");
// Repeated calls with the same path return the cached handle.
```

### Atlas Slicing

```rust
let mut tex = ctx.textures();
let tiles = tex.load_atlas("assets/atlas.png", 64, 64);
// tiles is Vec<TextureHandle>, one per tile in row-major order.
```

### Creating Texture Arrays

```rust
let mut tex = ctx.textures();
let t0 = tex.load("assets/wall_0.png");
let t1 = tex.load("assets/wall_1.png");
let array = tex.create_array(&[t0, t1]);
// All textures in the array must have identical dimensions.
```

### Procedural Textures

```rust
let mut tex = ctx.textures();
let data = vec![255u8; 64 * 64 * 4]; // Solid white 64x64
let handle = tex.from_rgba(data, 64, 64);
```

## Integration Points

### SceneContext

`SceneContext` holds `&mut TextureManager`, `&wgpu::Device`, and `&wgpu::Queue`. The `textures()` method constructs a `TextureContext` from these references, providing the primary API for game code.

### GpuRaycastRenderer

Stores `TextureArrayHandle` fields (`wall_textures`, `floor_textures`, `sprite_textures`). During rendering, looks up views via `texture_mgr.get_array_view(handle)` to build bind groups for the raycasting shader.

### SpriteGpuRenderer

Maintains a `bind_group_cache: HashMap<TextureHandle, wgpu::BindGroup>` and `id_to_handle: HashMap<String, TextureHandle>`. When a sprite is uploaded, its texture data is sent to the `TextureManager`, and the resulting handle is used to create and cache a bind group for rendering.

## Design Decisions

### Why Generational Handles

Raw indices can dangle when a texture is removed and the slot is reused. Generational handles include a `generation` counter that increments on removal. Stale handles fail lookups with `None` instead of silently referencing the wrong texture. The phantom type parameter `M` prevents accidentally using a `TextureHandle` where a `TextureArrayHandle` is expected (compile-time safety).

### Why Retained CPU Data

`ManagedTexture` keeps a `data: Vec<u8>` CPU copy of the texture pixels. This is necessary because `create_array()` must re-upload individual texture data into array layers, and wgpu does not support reading back from GPU textures efficiently. The CPU copy enables composing arrays from previously loaded individual textures without re-decoding from disk.

### Why Device/Queue Per-Method (TextureManager) vs. TextureContext

`TextureManager` methods require `device` and `queue` parameters because the manager does not own GPU state -- it may outlive individual frames and surface configurations. `TextureContext` exists as a convenience wrapper for scene callbacks where device/queue are always available, avoiding repetitive parameter threading.

### Why Renderers Own Bind Groups

Bind groups are tightly coupled to pipeline layouts, which vary per renderer. The raycast renderer needs `texture_2d_array` bindings while the sprite renderer needs `texture_2d` bindings. Keeping bind groups in renderers avoids a centralized system that would need to know every possible layout. The `TextureManager` stays focused on texture lifecycle; renderers handle GPU pipeline integration.

## Common Issues

### Stale Handle After Drop

If you call `drop_texture()` or `drop_array()` and then use the handle, `get_view()` returns `None`. Always check the return value or ensure handles are discarded after dropping.

### Array Dimension Mismatch

`create_array()` asserts all textures have identical dimensions. Mixing sizes will panic. When loading an atlas, all tiles are guaranteed to match, so atlas-to-array is always safe.

### Path Deduplication Scope

The path cache uses `PathBuf` keys, so `"assets/wall.png"` and `"./assets/wall.png"` are different entries. Canonicalize paths if needed.
