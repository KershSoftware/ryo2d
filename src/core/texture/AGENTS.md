# AGENTS.md - Texture Management

All content GPU textures (not render targets) are loaded, stored, and accessed through a single `TextureManager` owned by the engine. Renderers receive lightweight generational handles rather than raw GPU objects, enabling safe lifecycle management and deduplication.

## Design Decisions

### Why Generational Handles

Raw indices can dangle when a texture is removed and the slot is reused. Generational handles include a `generation` counter that increments on removal. Stale handles fail lookups with `None` instead of silently referencing the wrong texture. The phantom type parameter `M` prevents accidentally using a `TextureHandle` where a `TextureArrayHandle` is expected (compile-time safety).

### Why Retained CPU Data

`ManagedTexture` keeps a `data: Vec<u8>` CPU copy of the texture pixels. This is necessary because `create_array()` must re-upload individual texture data into array layers, and wgpu does not support reading back from GPU textures efficiently. The CPU copy enables composing arrays from previously loaded individual textures without re-decoding from disk.

### Why Device/Queue Per-Method (TextureManager) vs. TextureContext

`TextureManager` methods require `device` and `queue` parameters because the manager does not own GPU state -- it may outlive individual frames and surface configurations. `TextureContext` (from `ctx.textures()`) exists as a convenience wrapper for scene callbacks where device/queue are always available.

### Why Renderers Own Bind Groups

Bind groups are tightly coupled to pipeline layouts, which vary per renderer. The raycast renderer needs `texture_2d_array` bindings while the sprite renderer needs `texture_2d` bindings. Keeping bind groups in renderers avoids a centralized system that would need to know every possible layout. The `TextureManager` stays focused on texture lifecycle; renderers handle GPU pipeline integration.

## Common Issues

- **Bad path panics**: `load()` and `load_atlas()` panic if the image can't be opened/decoded. `load_atlas()` also panics if the image isn't evenly divisible by the tile size.
- **Stale handle after drop**: after `drop_texture()`/`drop_array()`, `get_view()` returns `None`. Passing a dropped handle to `create_array()` panics.
- **Array dimension mismatch**: `create_array()` asserts all textures have identical dimensions. Atlas tiles always match, so atlas-to-array is safe.
- **Path dedup scope**: the cache keys on `PathBuf`, so `"assets/wall.png"` and `"./assets/wall.png"` are different entries. Use consistent paths.
