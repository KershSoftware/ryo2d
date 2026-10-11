# AGENTS.md - Raycasting System

## Cross-File Flow

- ECS → GPU sync lives in `engine.rs` `sync_raycast()` (camera, sprites sorted back-to-front, lights, `flush_pending`)
- The `raycast` pass (`render/passes/raycast.rs`) draws walls and sprites; post-processing, UI and letterbox are the shared passes after it

## GPU Data Pipeline

1. **Textures** - The scene builds arrays with `ctx.textures().create_array(&handles)` and passes them to `set_{wall,floor,sprite}_textures(TextureArrayHandle)`. The arrays are owned by `TextureManager` (see `texture/AGENTS.md`)
2. **`flush_pending(device, queue, &TextureManager)`** - Called by the engine. Uploads map info, plus map data and lightmap when dirty, and rebuilds cached bind groups from manager array views
3. **ECS sync** - Camera, sprites and lights are written to GPU buffers
4. **Render** - Wall pass (DDA, fullscreen quad), then sprite pass (instanced billboards)

## Usage

Configure the renderer via `ctx.raycast_renderer()` (`set_world`, `set_map_info`, `set_*_textures`), then call `ctx.enable_raycast()` to switch from normal 2D rendering to the raycast path. See `examples/demos/raycasting/`.

## Design Decisions

- **GPU-only rendering** - The raycasting path bypasses the CPU pixel buffer entirely for performance
- **Texture arrays** - Wall/floor/sprite textures are `texture_2d_array`s from `TextureManager`, indexed by texture ID in the shader
- **f16 packing** - Map cell heights and light levels are packed as f16 to minimize GPU memory (3×u32 per cell)
- **Cached bind groups** - Built in `flush_pending()` and reused until textures change (`set_*_textures` invalidates them), avoiding per-frame allocation
- **SceneCleanup::RAYCAST** - Cleanup flag disables the renderer and drops the world map on scene transitions

## Notes for AI Agents

- The `MapCell::pack()` layout must stay in sync with the unpack functions in `raycast.wgsl`. See the cross-reference comments in both files.
- `SpriteInstance` field order matters for WGSL alignment: `vec2<f32>` fields must be on 8-byte boundaries.
- `LightInstance` uses `_pad0: f32, _pad1: f32, _pad2: f32` padding in WGSL (NOT `vec3<f32>`), because `vec3<f32>` has 16-byte alignment and would cause an array stride mismatch with the Rust struct.
- Sprites are sorted back-to-front by distance from the camera before GPU upload. The GPU sprite pass uses alpha blending, not depth testing between sprites.
- The lightmap is an R8Unorm texture (not R32Float) because R32Float requires the `FLOAT32_FILTERABLE` feature for bilinear sampling. R8Unorm is always filterable, and 256 brightness levels is enough.
- `PORTAL` and `TRANSPARENT` flags (and `Portal`) are defined but not yet implemented in the shader.
