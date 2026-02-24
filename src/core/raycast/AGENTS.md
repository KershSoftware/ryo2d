# AGENTS.md - Raycasting System

## Overview

This folder contains the GPU-accelerated raycasting system for pseudo-3D rendering (Wolfenstein/Build engine style). The renderer uses two WGSL shader passes — a DDA wall raycaster and an instanced billboard sprite renderer — composed into the engine's existing post-processing and UI pipeline.

## Architecture

```
ECS World (entities with Transform + RaycastCamera/Sprite/Light/Door)
       │
       ▼
  Engine sync_and_render_raycast()
       │
       ├─ Camera sync  ──► CameraUniform ──► GPU camera buffer
       ├─ Sprite sync   ──► SpriteInstance[] ──► GPU sprite buffer (sorted back-to-front)
       ├─ Light sync    ──► LightInstance[] ──► GPU light buffer
       ├─ World map     ──► pack_for_gpu()  ──► GPU map storage buffer
       └─ Lightmap      ──► generate_lightmap() ──► R8Unorm texture (bilinear sampled)
              │
              ▼
  GpuRaycastRenderer
       │
       ├─ Wall pass (raycast.wgsl)     ──► game texture + depth texture
       └─ Sprite pass (raycast_sprite.wgsl) ──► game texture (reads depth)
              │
              ▼
  WgpuRenderer::render_raycast()
       │
       ├─ Post-processing effects
       ├─ UI buffer compositing
       └─ Letterbox/scaling to screen
```

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Public exports for raycasting types |
| `components.rs` | ECS components: `RaycastCamera`, `RaycastSprite`, `RaycastLight`, `RaycastDoor` |
| `world.rs` | `MapCell`, `MapFlags`, `RaycastWorld` grid with GPU packing |
| `systems.rs` | `door_system()`, `camera_follow_system()`, GPU uniform structs |
| `gpu.rs` | `GpuRaycastRenderer` — pipelines, buffers, textures, bind groups |

## Key Types

### Components (`components.rs`)

- **RaycastCamera** — Direction vector, FOV plane, eye height. Attached to one entity with a `Transform`.
- **RaycastSprite** — Texture ID, scale, vertical offset. Billboard rendered in world space.
- **RaycastLight** — Radius, RGB colour, intensity, height, falloff exponent. Dynamic point lighting with configurable attenuation curves.
- **RaycastDoor** — Map position, state machine (Closed→Opening→Open→Closing), speed, auto-close timer.

### World Map (`world.rs`)

- **MapCell** — Per-tile data: wall/floor/ceiling texture indices, flags, heights, door state, light level.
- **MapFlags** — Bitflags: `DOOR`, `THIN_WALL`, `PORTAL` (not yet implemented), `TRANSPARENT` (not yet implemented).
- **RaycastWorld** — Row-major grid with dirty tracking. `pack_for_gpu()` converts to 3×u32 per cell.
- **Portal** — Source/destination cell pairs (not yet implemented in shader).

### GPU Uniforms (`systems.rs`)

| Struct | Size | Purpose |
|--------|------|---------|
| `CameraUniform` | 48B | Camera pos, dir, plane, screen size, height, time |
| `MapInfoUniform` | 48B | Map dimensions, max distance, fog density/colour, diminish factor, ambient colour/intensity |
| `SpriteInstance` | 32B | Per-sprite pos, scale, texture ID, vertical offset |
| `LightInstance` | 48B | Per-light pos, radius, intensity, colour, height, falloff exponent |

### GPU Renderer (`gpu.rs`)

- **GpuRaycastRenderer** — Owns wall and sprite pipelines, all buffers, texture arrays, cached bind groups.
- Bind groups are cached and rebuilt only when textures change.
- Depth texture (R32Float) written by wall pass, read by sprite pass for occlusion.

## GPU Data Pipeline

1. **Texture upload** — `queue_wall_textures()` / `queue_floor_textures()` / `queue_sprite_textures()` store raw RGBA data.
2. **flush_pending()** — Creates `texture_2d_array` resources, uploads map data and lightmap texture when dirty, rebuilds cached bind groups.
3. **ECS sync** — Engine helper `sync_and_render_raycast()` collects camera, sprites (sorted back-to-front), and lights from ECS, writes to GPU buffers.
4. **Render** — Wall pass renders fullscreen quad with DDA raycasting; sprite pass renders instanced billboards.

## Render Pipeline

### Wall Pass (`shaders/core/raycast.wgsl`)
- Fullscreen quad (6 vertices, no vertex buffer)
- Per-pixel DDA ray marching through the 2D tile map
- Textured walls, floors, ceilings
- Door handling (ray passes through open portion)
- Six-layer lighting: bilinear lightmap ambient → global ambient → dynamic point lights (with 3D height and configurable falloff) → shadow rays → camera-distance diminishing → distance fog
- Outputs: colour to game texture, depth to R32Float texture

### Sprite Pass (`shaders/core/raycast_sprite.wgsl`)
- Instanced rendering (6 vertices × N sprites)
- Camera-space projection of world-space positions
- Alpha testing with discard
- Depth testing against wall depth buffer
- Full dynamic lighting pipeline: lightmap ambient → global ambient → point lights → diminishing (shared light buffer with wall pass)

## Usage Pattern

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
    let world = RaycastWorld::new(32, 32);
    // ... populate cells ...
    let rc = ctx.raycast_renderer().unwrap();
    rc.set_world(world);
    rc.set_map_info(MapInfoUniform { ... });
    rc.queue_wall_textures(images, 64, 64);
    rc.queue_floor_textures(images, 64, 64);
    rc.queue_sprite_textures(images, 64, 64);
    ctx.enable_raycast();

    ctx.world.spawn((Transform::new(x, y), RaycastCamera::new(66.0)));
    ctx.world.spawn((Transform::new(x, y), RaycastSprite::new(0)));
    ctx.world.spawn((Transform::new(x, y), RaycastLight::new(5.0, (1.0, 0.8, 0.6), 1.5)));
}
```

## Design Decisions

- **GPU-only rendering** — The raycasting path bypasses the CPU pixel buffer entirely for performance.
- **Texture arrays** — All wall/floor/sprite textures are packed into `texture_2d_array` resources, indexed by texture ID in the shader.
- **f16 packing** — Map cell heights and light levels are packed as f16 to minimize GPU memory (3×u32 per cell).
- **Cached bind groups** — Bind groups are built once in `flush_pending()` and reused until textures change, avoiding per-frame allocation.
- **SceneCleanup::RAYCAST** — Cleanup flag disables renderer and drops world map on scene transitions.

## Notes for AI Agents

- The `MapCell::pack()` layout must stay in sync with the unpack functions in `raycast.wgsl`. See cross-reference comments in both files.
- `SpriteInstance` field order matters for WGSL alignment — `vec2<f32>` fields must be on 8-byte boundaries.
- `LightInstance` uses `_pad0: f32, _pad1: f32, _pad2: f32` padding in WGSL (NOT `vec3<f32>`) because `vec3<f32>` has 16-byte alignment which would cause array stride mismatch with the Rust struct.
- Sprites are sorted back-to-front by distance from camera before GPU upload. The GPU sprite pass uses alpha blending, not depth testing between sprites.
- The lightmap is an R8Unorm texture (not R32Float) because R32Float requires the `FLOAT32_FILTERABLE` feature for bilinear sampling. R8Unorm is always filterable and 256 brightness levels is sufficient.
- `PORTAL` and `TRANSPARENT` flags are defined but not yet implemented in the shader.
