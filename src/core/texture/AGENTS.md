# AGENTS.md - Texture Management

`TextureManager` (a resource) loads, stores and hands out content GPU textures (not render targets) through generational handles, for code that binds textures itself: a game's `RenderPass` or a future renderer. Scenes use `ctx.textures()`, a `TextureContext` over the manager and the `Gpu` resource.

It is not the sprite path. Sprites are `SpriteHandle`s from `AssetManager`, packed into the sprite atlas by `SpriteGpuRenderer` (`src/core/graphics/AGENTS.md`). Nothing in the engine draws `TextureManager` textures today: their one consumer, the raycaster, is parked in `attic/raycast/` and returns on the new render pipeline.

## Design Decisions

### Why Generational Handles

Raw indices can dangle when a texture is removed and the slot is reused. Handles carry a `generation` that increments on removal, so a stale handle fails lookups with `None` instead of silently referencing the wrong texture. The phantom marker type keeps a `TextureHandle` from being used where a `TextureArrayHandle` is expected.

### Why Retained CPU Data

`ManagedTexture` keeps a CPU copy of its pixels: `create_array()` re-uploads individual textures into array layers, and reading back from the GPU is slow. The copy composes arrays from loaded textures without decoding the files again.

### Why Device/Queue per Method

`TextureManager` methods take `device` and `queue` because the manager does not own GPU state. `TextureContext` holds a clone of the `Gpu` resource so scene code needs neither. The `Gpu` resource exists once the GPU is up, before the first scene enters; `ctx.textures()` panics without it.

### Why Renderers Own Bind Groups

Bind groups are tied to pipeline layouts, which vary per renderer (`texture_2d` for one, `texture_2d_array` for another). The manager stays focused on texture lifecycle (`get_view`, `get_array_view`, `dimensions`); whoever draws builds its own bind groups.

## Common Issues

- **Bad path panics**: `load()` and `load_atlas()` panic if the image cannot be opened or decoded. `load_atlas()` also panics if the image is not evenly divisible by the tile size.
- **Stale handle after drop**: after `drop_texture()` / `drop_array()`, `get_view()` returns `None`. Passing a dropped handle to `create_array()` panics, as does an empty handle list.
- **Array dimension mismatch**: `create_array()` asserts all textures have identical dimensions. Atlas tiles always match, so atlas-to-array is safe.
- **Path dedup scope**: the cache keys on `PathBuf`, so `"assets/wall.png"` and `"./assets/wall.png"` are different entries. Use consistent paths.
