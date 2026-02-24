# TextureManager Design

## Goal

Centralize GPU content texture ownership into a single `TextureManager`, replacing per-renderer texture management in `SpriteGpuRenderer` and `GpuRaycastRenderer`. Provides loading, atlas slicing, deduplication, lifecycle management, and a unified API for all rendering subsystems.

## Motivation

The engine currently has 5 subsystems independently managing GPU textures:

| Owner | Textures | Type |
|-------|----------|------|
| `SpriteGpuRenderer` | Per-sprite texture cache | Individual `texture_2d` |
| `GpuRaycastRenderer` | Wall/floor/sprite arrays | `texture_2d_array` |
| `PostProcessPipeline` | Ping-pong + bloom buffers | Render targets |
| `LightingRenderer` | Light accumulation buffer | HDR render target |
| `WgpuRenderer` | Game/UI/CPU framebuffers | Render targets |

Problems with current state:
- No deduplication — same image can be uploaded to GPU multiple times
- No shared loading — each renderer loads/manages textures independently
- No atlas slicing — raycasting demo creates textures procedurally because there's no utility to slice atlas images into `Vec<Vec<u8>>`
- Scattered lifecycle management — texture cleanup is per-renderer

## Scope

**In scope (content textures):**
- Images loaded from disk (PNG, JPEG)
- Procedurally generated textures (raw RGBA bytes)
- Atlas slicing (grid-based image → individual tile textures)
- Texture arrays (individual textures → `texture_2d_array`)

**Out of scope (render targets stay with their pipelines):**
- Post-process ping-pong buffers (PostProcessPipeline)
- Lighting accumulation buffers (LightingRenderer)
- Depth textures (GpuRaycastRenderer)
- Game/UI framebuffers (WgpuRenderer)

Render targets are internal pipeline implementation details, not content. Centralizing them adds complexity with no benefit.

## Architecture

### Module Structure

```
src/core/texture/
├── mod.rs          // Public exports
├── manager.rs      // TextureManager core logic
├── handle.rs       // Generational handle types + slot allocator
└── atlas.rs        // Atlas slicing utility
```

### Ownership Chain

```
Engine
├── WgpuRenderer
│   ├── device: wgpu::Device
│   ├── queue: wgpu::Queue
│   └── SpriteGpuRenderer (bind_group_cache only)
├── TextureManager (owns all content GPU textures)
├── GpuRaycastRenderer (TextureArrayHandles + bind_group_cache)
├── PostProcessPipeline (render targets, unchanged)
└── LightingRenderer (render targets, unchanged)
```

TextureManager lives as a sibling to WgpuRenderer in Engine. SceneContext builds a `TextureContext` by borrowing `&mut texture_manager` + `&device` + `&queue` together.

### Relationship to AssetManager

AssetManager and TextureManager coexist:
- **AssetManager** — CPU-side asset management (sprites as `Vec<u8>`, sounds, fonts, configs)
- **TextureManager** — GPU texture management (wgpu textures, views, arrays)

They serve different layers of the pipeline and don't interfere.

## Key Types

### Handles (handle.rs)

Generational IDs that detect use-after-free when textures are deleted and slots reused:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureHandle {
    index: u32,
    generation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureArrayHandle {
    index: u32,
    generation: u32,
}
```

### Slot Allocator (handle.rs)

```rust
struct Slot<T> {
    value: Option<T>,
    generation: u32,
}
```

When a texture is deleted: slot value → None, generation incremented. Stale handles (wrong generation) return None on access.

### Managed Resources (manager.rs)

```rust
struct ManagedTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    data: Vec<u8>,  // CPU copy retained for array building
}

struct ManagedTextureArray {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    layer_count: u32,
}
```

CPU data is retained in `ManagedTexture` because `create_array()` needs to copy pixel data from individual handles into `texture_2d_array` layers. Memory cost is small (16KB per 64x64 RGBA texture).

## TextureManager API (manager.rs)

Following the existing engine pattern (e.g., `SpriteGpuRenderer::upload_texture()`), methods take `&Device` and `&Queue` as parameters.

```rust
pub struct TextureManager {
    textures: Vec<Slot<ManagedTexture>>,
    arrays: Vec<Slot<ManagedTextureArray>>,
    free_texture_slots: Vec<u32>,
    free_array_slots: Vec<u32>,
    path_cache: HashMap<PathBuf, TextureHandle>,
}

impl TextureManager {
    pub fn new() -> Self;

    // --- Loading ---

    /// Load image from disk. Deduplicates by canonical path.
    pub fn load(&mut self, path: &Path,
                device: &Device, queue: &Queue) -> TextureHandle;

    /// Create from raw RGBA bytes (procedural textures).
    pub fn from_rgba(&mut self, data: Vec<u8>, width: u32, height: u32,
                     device: &Device, queue: &Queue) -> TextureHandle;

    /// Load atlas image and slice into individual tile textures.
    /// Simple grid slicing: no spacing or margin.
    pub fn load_atlas(&mut self, path: &Path, tile_w: u32, tile_h: u32,
                      device: &Device, queue: &Queue) -> Vec<TextureHandle>;

    // --- Arrays (for raycasting) ---

    /// Build a texture_2d_array from individual texture handles.
    /// All referenced textures must have the same dimensions.
    pub fn create_array(&mut self, handles: &[TextureHandle],
                        device: &Device, queue: &Queue) -> TextureArrayHandle;

    // --- Access ---

    pub fn get_view(&self, handle: TextureHandle) -> Option<&TextureView>;
    pub fn get_array_view(&self, handle: TextureArrayHandle) -> Option<&TextureView>;
    pub fn dimensions(&self, handle: TextureHandle) -> Option<(u32, u32)>;

    // --- Lifecycle ---

    pub fn drop_texture(&mut self, handle: TextureHandle);
    pub fn drop_array(&mut self, handle: TextureArrayHandle);
}
```

### Deduplication

`load()` canonicalizes the path and checks `path_cache`. If a handle already exists for that path, it returns the existing handle (no GPU upload). `from_rgba()` and `load_atlas()` do not deduplicate (procedural textures have no path key).

### Atlas Slicing (atlas.rs)

Simple grid slicing — divides image evenly by `tile_w × tile_h`:

```rust
/// Slice raw RGBA image data into individual tile buffers.
pub fn slice_atlas(
    data: &[u8],
    img_width: u32,
    img_height: u32,
    tile_width: u32,
    tile_height: u32,
) -> Vec<Vec<u8>>
```

Row-major order (left-to-right, top-to-bottom). Spacing and margin support can be added later via a `load_atlas_with_options()` variant.

## Game Developer API

### TextureContext (convenience wrapper)

Game developers access TextureManager through SceneContext without manually threading `device`/`queue`:

```rust
pub struct TextureContext<'a> {
    manager: &'a mut TextureManager,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
}

impl<'a> TextureContext<'a> {
    pub fn load(&mut self, path: &str) -> TextureHandle;
    pub fn load_atlas(&mut self, path: &str, tile_w: u32, tile_h: u32) -> Vec<TextureHandle>;
    pub fn from_rgba(&mut self, data: Vec<u8>, w: u32, h: u32) -> TextureHandle;
    pub fn create_array(&mut self, handles: &[TextureHandle]) -> TextureArrayHandle;
}
```

### Usage Example (raycasting scene)

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
    let mut tex = ctx.textures();

    // Load wall atlas (512x512 image → 64 tiles of 64x64)
    let wall_tiles = tex.load_atlas("assets/walls.png", 64, 64);
    let wall_array = tex.create_array(&wall_tiles);

    // Load individual sprite textures
    let barrel = tex.load("assets/barrel.png");
    let torch = tex.load("assets/torch.png");
    let sprite_array = tex.create_array(&[barrel, torch]);

    // Assign to raycaster
    let rc = ctx.raycast_renderer().unwrap();
    rc.set_wall_textures(wall_array);
    rc.set_sprite_textures(sprite_array);
}
```

## Renderer Integration

### SpriteGpuRenderer Changes

**Before:** Owns `texture_cache: HashMap<String, CachedTexture>` where `CachedTexture` holds `wgpu::Texture` + `TextureView` + `BindGroup`.

**After:** Drops texture ownership. Keeps bind group cache keyed by `TextureHandle`:

```rust
// Before
struct CachedTexture {
    texture: wgpu::Texture,      // REMOVED — owned by TextureManager
    view: wgpu::TextureView,     // REMOVED — owned by TextureManager
    bind_group: wgpu::BindGroup, // KEPT
    width: u32,                  // REMOVED — query TextureManager
    height: u32,                 // REMOVED — query TextureManager
}

// After
bind_group_cache: HashMap<TextureHandle, wgpu::BindGroup>
```

Bind groups are pipeline-specific (tied to layout) so they stay in the renderer.

### GpuRaycastRenderer Changes

**Before:** Owns `wall_texture_array`, `floor_texture_array`, `sprite_texture_array` as `Option<wgpu::Texture>` + `Option<wgpu::TextureView>`, plus `pending_*_tex` buffers.

**After:** Stores `TextureArrayHandle`s. No pending buffers (TextureManager uploads eagerly):

```rust
// Before
pending_wall_tex: Option<(Vec<Vec<u8>>, u32, u32)>,
wall_texture_array: Option<wgpu::Texture>,
wall_texture_array_view: Option<wgpu::TextureView>,

// After
wall_textures: Option<TextureArrayHandle>,
floor_textures: Option<TextureArrayHandle>,
sprite_textures: Option<TextureArrayHandle>,
```

The `queue_*_textures() → flush_pending()` pattern is replaced by direct `set_*_textures(handle)` calls.

## Design Decisions

1. **Generational handles over string IDs** — Catches stale references at runtime instead of silently using wrong textures.

2. **Retained CPU data** — Needed for `create_array()` which must copy pixel data into array layers. Memory cost is negligible for typical game textures.

3. **device/queue per-method** — Matches existing engine patterns. Avoids `Arc` wrapping of wgpu resources. `TextureContext` wrapper provides ergonomic API for game developers.

4. **Renderers own bind groups** — Bind groups are tied to pipeline layouts. Centralizing them in TextureManager would couple it to every pipeline's bind group layout. Clean separation: TextureManager manages textures + views, renderers manage bind groups.

5. **Simple atlas slicing** — No spacing/margin support initially. Covers the immediate use case (clean grid atlases for raycasting). Easy to extend later.

6. **Coexist with AssetManager** — Different layers of the pipeline. AssetManager handles CPU-side assets, TextureManager handles GPU textures. No overlap, no coupling.

## Migration Plan

Each phase produces a working engine. No big-bang migration.

### Phase 1: Core TextureManager
- Implement `handle.rs` (generational handles, slot allocator)
- Implement `atlas.rs` (slice_atlas utility)
- Implement `manager.rs` (TextureManager with all methods)
- Unit tests for handle allocation/deallocation, atlas slicing, dedup
- Wire into Engine struct (owned field, not yet used by renderers)

### Phase 2: GpuRaycastRenderer Integration
- Replace `pending_*_tex` + `*_texture_array` fields with `TextureArrayHandle`
- Replace `queue_*_textures()` + `flush_pending()` with `set_*_textures(handle)`
- Update bind group caching to use TextureManager views
- Update raycasting demo to use TextureManager (load atlas images)
- Verify raycasting rendering still works

### Phase 3: SpriteGpuRenderer Integration
- Replace `texture_cache: HashMap<String, CachedTexture>` with `HashMap<TextureHandle, BindGroup>`
- Update `upload_texture()` to accept `TextureHandle` + `&TextureManager`
- Update engine sprite upload path to load through TextureManager
- Verify all 2D sprite rendering still works

### Phase 4: SceneContext API
- Add `TextureContext` convenience wrapper
- Add `ctx.textures()` method to SceneContext
- Update existing demos/games that load textures
- Documentation (AGENTS.md, mdbook chapter)
