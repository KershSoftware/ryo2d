# Raycasting & Pseudo-3D Rendering for Ryo2D

Research document covering raycasting techniques for implementing 3D-in-2D game rendering, with concrete integration strategies for the Ryo2D engine.

---

## Table of Contents

1. [Classic Raycasting (Wolfenstein 3D Style)](#1-classic-raycasting-wolfenstein-3d-style)
2. [Mode 7 Style Rendering](#2-mode-7-style-rendering)
3. [GPU-Accelerated Raycasting](#3-gpu-accelerated-raycasting)
4. [Advanced Features](#4-advanced-features)
5. [Ryo2D Integration Strategy](#5-ryo2d-integration-strategy)
6. [Existing Rust Implementations](#6-existing-rust-implementations)
7. [References](#7-references)

---

## 1. Classic Raycasting (Wolfenstein 3D Style)

### 1.1 The DDA (Digital Differential Analyzer) Algorithm

The DDA algorithm is the foundation of Wolfenstein 3D-style raycasting. The world is a 2D grid where each cell is either empty (0) or a wall (positive integer = texture ID). For every vertical column of pixels on screen, one ray is cast from the player's position into the scene.

Rather than stepping along the ray at tiny fixed intervals (slow, imprecise), DDA steps from one grid line intersection to the next — only checking positions where a wall boundary could exist.

**Algorithm steps:**

**Step 1 — Ray direction.** For each screen column `x` (0 to `screen_width`), compute a ray direction by interpolating across the camera plane:

```
cameraX = 2 * x / screen_width - 1        // -1 (left) to +1 (right)
rayDirX = dirX + planeX * cameraX
rayDirY = dirY + planeY * cameraX
```

`(dirX, dirY)` is the player's look direction. `(planeX, planeY)` is the camera plane vector (perpendicular to direction; its length determines FOV — length 0.66 ≈ 66° FOV).

**Step 2 — Delta distance.** How far the ray must travel to cross one full grid cell in each axis:

```
deltaDist.x = abs(1.0 / rayDir.x)
deltaDist.y = abs(1.0 / rayDir.y)
```

**Step 3 — Initial side distance.** Distance from the player's position to the first grid boundary in each axis:

```
if rayDir.x < 0:
    stepX = -1
    sideDistX = (posX - mapX) * deltaDist.x
else:
    stepX = 1
    sideDistX = (mapX + 1.0 - posX) * deltaDist.x
// (same for Y)
```

**Step 4 — DDA loop.** Step through the grid, always advancing along whichever axis has the shorter side distance:

```
loop:
    if sideDistX < sideDistY:
        sideDistX += deltaDist.x
        mapX += stepX
        side = 0    // hit an X-side (vertical wall face)
    else:
        sideDistY += deltaDist.y
        mapY += stepY
        side = 1    // hit a Y-side (horizontal wall face)
    if worldMap[mapX][mapY] > 0:
        break       // wall hit
```

**Step 5 — Perpendicular wall distance (fisheye-free):**

```
if side == 0:
    perpWallDist = sideDistX - deltaDist.x
else:
    perpWallDist = sideDistY - deltaDist.y
```

By using perpendicular distance (distance to the camera plane, not Euclidean distance from the player), the fisheye effect is inherently avoided without any separate correction step.

### 1.2 Wall Rendering with Vertical Strips

Once `perpWallDist` is known for a column:

```
lineHeight = (int)(screenHeight / perpWallDist)
drawStart = -lineHeight / 2 + screenHeight / 2
drawEnd = lineHeight / 2 + screenHeight / 2
```

Each column is drawn as a single vertical strip of this height, centered on screen. Shorter distances → taller strips (closer walls), longer distances → shorter strips (distant walls).

### 1.3 Texture Mapping on Walls

The texture X-coordinate (`wallX`) comes from the exact world-space hit position:

```
if side == 0:
    wallX = posY + perpWallDist * rayDirY
else:
    wallX = posX + perpWallDist * rayDirX
wallX -= floor(wallX)       // fractional part only
texX = (int)(wallX * texWidth)
```

For vertical sampling, each pixel in the wall strip maps to a texture Y-coordinate via linear interpolation. This is affine texture mapping, which is perspective-correct for perfectly vertical surfaces:

```
step = texHeight / lineHeight
texPos = (drawStart - screenHeight/2 + lineHeight/2) * step
for each y in drawStart..drawEnd:
    texY = (int)texPos & (texHeight - 1)
    texPos += step
    pixel = texture[texX][texY]
```

### 1.4 Floor and Ceiling Casting

Floors and ceilings use **horizontal scanlines** rather than vertical strips.

For each row `y` from `screenHeight/2 + 1` to `screenHeight`:

1. Compute ray direction for leftmost and rightmost screen columns at this row.
2. Compute row distance: `rowDistance = cameraHeight / (y - screenHeight / 2)`
3. Compute real-world floor positions for left/right edges of the scanline:
   ```
   floorStepX = rowDistance * (rayDirX_right - rayDirX_left) / screenWidth
   floorStepY = rowDistance * (rayDirY_right - rayDirY_left) / screenWidth
   floorX = playerX + rowDistance * rayDirX_left
   floorY = playerY + rowDistance * rayDirY_left
   ```
4. For each pixel `x` in the scanline, look up the floor texture at `(floorX, floorY)`, then increment by `(floorStepX, floorStepY)`.
5. Ceiling is the mirror: draw the same coordinates at row `screenHeight - y - 1`.

This scanline interpolation is perspective-correct because the floor is a perfectly horizontal plane.

### 1.5 Fisheye Correction — Three Approaches

**Approach A — Perpendicular distance (preferred).** Compute only the perpendicular distance to the camera plane. This naturally avoids fisheye and is simpler. The formula `perpWallDist = sideDistX - deltaDist.x` directly yields the correct value. Used by Lode's tutorial.

**Approach B — Cosine correction.** If rays are cast at equal angular intervals, multiply the Euclidean distance by `cos(rayAngle - playerAngle)`. Works but introduces subtle horizontal stretching.

**Approach C — Projection plane (most correct).** Space rays so they pass through evenly-spaced points on a projection plane. Avoids both fisheye and horizontal distortion. This is effectively what Lode's camera-plane method does.

---

## 2. Mode 7 Style Rendering

### 2.1 Core Concept

Mode 7 (SNES-era pseudo-3D) applies a 2D affine transformation per-scanline to a tilemap, creating the illusion of a ground plane receding into the distance. Games like Mario Kart, F-Zero, and Pilotwings used this technique.

### 2.2 The Affine Transformation

Screen pixel `(Sx, Sy)` maps to texture coordinate `(Tx, Ty)`:

```
| Tx |   | M7A  M7B |   | Sx + HOFS - M7X |   | M7X |
|    | = |          | x |                 | + |     |
| Ty |   | M7C  M7D |   | Sy + VOFS - M7Y |   | M7Y |
```

- `(M7A, M7B, M7C, M7D)` = transformation coefficients (8.8 fixed-point on SNES)
- `(M7X, M7Y)` = pivot/rotation center
- `(HOFS, VOFS)` = scroll offsets

### 2.3 How Perspective Is Simulated

Since affine transforms cannot do true perspective, the SNES achieves it by **changing matrix parameters every scanline** (via HDMA):

- Bottom scanlines (foreground): small scaling factor → nearby texels
- Top scanlines (horizon): large scaling factor → distant texels
- Scale formula: `lambda = camera_height / h` where `h` = distance from screen center

The per-scanline math from Tonc's tutorial:

```
For scanline h (distance from center):
    lambda = camera_y / h
    a_cx = lambda * cos(alpha)      // M7A
    a_cy = lambda * sin(alpha)      // M7B (negated for correct rotation)
```

### 2.4 Modern Implementation Approaches

**Fragment shader (faithful):** Write a shader that computes per-scanline affine transforms to look up a texture. Exactly replicates the SNES behavior including the characteristic affine texture warping.

**3D plane (simpler but different):** Render a textured 3D quad with a perspective camera. Produces perspective-correct texture mapping rather than the original affine "wobble." To replicate the authentic look, deliberately use affine interpolation.

---

## 3. GPU-Accelerated Raycasting

### 3.1 Fragment Shader Approach (Fullscreen Quad)

The simplest GPU architecture:

1. Render a fullscreen quad (two triangles covering the screen)
2. Fragment shader runs once per pixel, performing the entire DDA raycasting
3. Map data is stored in a 2D texture or storage buffer
4. Wall textures packed into a texture atlas

Ryo2D already has a fullscreen quad shader at `src/core/shaders/core/fullscreen_quad.wgsl` that generates the quad geometry procedurally from `vertex_index`. A raycasting fragment shader would follow this same pattern.

### 3.2 Compute Shader Approach

More flexible but requires two passes:

1. Compute shader dispatched at `(screen_width, screen_height, 1)` writes to a storage texture
2. Fragment shader on fullscreen quad reads and displays the result

Advantages: shared workgroup memory for caching map tiles, arbitrary dispatch sizes, flexible for complex effects.

### 3.3 Branchless DDA on GPU

GPUs handle branches poorly. A branchless DDA variant eliminates most conditionals:

- Use `step()`, `sign()`, `floor()` builtins instead of `if`
- Select axis advancement via math rather than branching
- The Shadertoy "Branchless Voxel Raycasting" (shadertoy.com/view/4dX3zl) demonstrates this

### 3.4 WGSL Implementation Skeleton

```wgsl
struct Camera {
    pos: vec2<f32>,
    dir: vec2<f32>,
    plane: vec2<f32>,
    screen_size: vec2<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> map_data: array<u32>;
@group(0) @binding(2) var wall_textures: texture_2d_array<f32>;
@group(0) @binding(3) var wall_sampler: sampler;

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let x = frag_coord.x;
    let y = frag_coord.y;
    let camera_x = 2.0 * x / camera.screen_size.x - 1.0;
    let ray_dir = camera.dir + camera.plane * camera_x;

    // DDA initialization
    var map_pos = vec2<i32>(floor(camera.pos));
    let delta_dist = abs(vec2<f32>(1.0) / ray_dir);

    // ... DDA loop, wall hit, texture lookup ...

    return texel_color;
}
```

Key WGSL considerations:
- No recursion allowed (not a problem for DDA, which is loop-based)
- `textureLoad()` reads individual texels; `textureSample()` does filtered reads
- Recommended workgroup size of 64 for compute shaders
- Max default storage buffer is 128 MB

---

## 4. Advanced Features

### 4.1 Sprite Rendering (Billboard Sprites)

Sprites are always-facing-camera 2D images rendered after walls using a 1D Z-buffer.

> Note: this section predates the current `RenderPipeline` (ordered passes, no dependencies or topological sorting); see `src/core/render` for the actual API.

**Pipeline:**

1. Sort all sprites far-to-near (painter's algorithm)
2. Transform each sprite into camera space:
   ```
   spriteX = sprite.x - player.x
   spriteY = sprite.y - player.y
   invDet = 1.0 / (planeX * dirY - dirX * planeY)
   transformX = invDet * (dirY * spriteX - dirX * spriteY)
   transformY = invDet * (-planeY * spriteX + planeX * spriteY)    // depth
   ```
3. Compute screen position/size:
   ```
   spriteScreenX = (int)(screenWidth / 2 * (1 + transformX / transformY))
   spriteHeight = abs((int)(screenHeight / transformY))
   spriteWidth = abs((int)(screenHeight / transformY))
   ```
4. Draw column-by-column, clipping against 1D Z-buffer from wall rendering:
   ```
   for stripe in drawStartX..drawEndX:
       if transformY > 0 && transformY < zBuffer[stripe]:
           // draw this column of the sprite
   ```

**Directional sprites (8-angle):** Calculate angle between player and sprite using `atan2(dy, dx)`, select one of 8 pre-rendered textures based on rounded angle.

### 4.2 Lighting

**Distance-based dimming (simplest):**
```
intensity = object_intensity / (distance * multiplier)
final_color = base_color * intensity
```

**Side-based shading (Wolfenstein 3D trick):** Walls on one axis darker than the other. Zero cost, gives directionality.

**Fog effect:**
```
fog_intensity = clamp(distance / fog_distance, 0, 1)
final_color = mix(texel_color, fog_color, fog_intensity)
```

Exponential fog: `fog_intensity = 1.0 - exp(-distance * density)` (Beer's law, more natural falloff).

**Per-tile lighting:** Each map tile has a light value modulating wall/floor brightness. The `raycaster` crate supports this.

### 4.3 Thin Walls and Portals

**Thin walls:** Line segments within a grid cell. When a ray hits a thin wall:
1. Record distance/texture in a "transparent wall cache"
2. Continue DDA stepping
3. Draw in reverse order after solid wall hit (painter's algorithm)

**Portal rendering:** When a ray hits a portal wall, continue from the portal destination rather than stopping. Performance self-regulates — portals cover fewer pixels at greater distances.

**Sector-based portals (Doom/Duke Nukem style):** Map divided into sectors with portal walls between them. Allows rooms with different floor/ceiling heights.

### 4.4 Multi-Height Floors/Ceilings (Build Engine Style)

The Build engine (Duke Nukem 3D) extended raycasting with per-sector floor/ceiling heights:

- Each sector has its own `floor_height` and `ceiling_height`
- Height differences between adjacent sectors create upper/lower wall segments
- A "height buffer" tracks min/max drawn Y per column
- Enables stairs, platforms, varying room heights

### 4.5 Transparent Walls

**Painter's algorithm approach:** Continue DDA past transparent walls, render solid first, then transparent layers far-to-near with blending.

**Z-buffer approach:** Full 2D Z-buffer (one depth per pixel). Draw pixels only if distance < stored value. Eliminates sorting but costs more memory.

---

## 5. Ryo2D Integration Strategy

### 5.1 Engine Architecture Overview

Ryo2D uses a pass-based render pipeline (`RenderPipeline` in `src/core/render/pipeline.rs`) with topological ordering. Each pass implements `RenderPass` (`src/core/render/pass.rs`) and receives a `RenderContext` providing access to:

- **GPU resources:** `device`, `queue`, `encoder`
- **Textures:** `game_texture`, `game_texture_view`, scratch buffers for ping-pong
- **CPU buffer:** `pixel_buffer: &mut [u8]`
- **Frame info:** `width`, `height`, `time`, `delta_time`
- **Camera:** `camera` (2D camera with world-to-screen transforms)

Current pipeline order:
```
ClearPass → ParallaxPass → CpuSpritePass → UploadPass → GpuSpritePass →
TrailPass → LightingPass → UiPass → EffectsPass → PresentPass
```

### 5.2 Three Integration Options

#### Option A — CPU Raycasting (Software Renderer)

Write raycasting as a custom render pass that writes directly to `pixel_buffer`.

```rust
pub struct RaycastPass {
    enabled: bool,
    map: RaycastMap,
    // ... camera state, textures
}

impl RenderPass for RaycastPass {
    fn id(&self) -> RenderPassId { "raycast" }

    fn dependencies(&self) -> &[RenderPassId] {
        &[pass_ids::CLEAR]  // Run after clear, before upload
    }

    fn execute(&mut self, ctx: &mut RenderContext) {
        // For each screen column x:
        //   1. Compute ray direction from camera
        //   2. Run DDA loop against self.map
        //   3. Compute wall height and texture coordinates
        //   4. Write pixels to ctx.pixel_buffer
        // Then render floors/ceilings via scanlines
    }
}
```

**Pipeline insertion:** After `ClearPass`, before `UploadPass`. The existing `UploadPass` transfers `pixel_buffer` to the GPU automatically.

**Pros:**
- Simplest to implement — works within existing CPU rendering path
- Full control over every pixel
- Can use `rayon` for multi-threaded column rendering (the `raycaster` crate achieves ~2-3ms at 1280x800 this way)
- Matches the pattern of the existing `CpuSpritePass`

**Cons:**
- CPU-bound; limited by memory bandwidth
- No GPU acceleration for the raycasting itself

#### Option B — GPU Raycasting (Shader-Based)

Create a GPU render pass that runs a fullscreen-quad fragment shader performing DDA.

```rust
pub struct GpuRaycastPass {
    enabled: bool,
    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,     // Camera uniform
    map_buffer: wgpu::Buffer,        // Storage buffer for map grid
    texture_atlas: wgpu::Texture,    // Wall textures
    bind_group: wgpu::BindGroup,
}

impl RenderPass for GpuRaycastPass {
    fn id(&self) -> RenderPassId { "gpu_raycast" }

    fn dependencies(&self) -> &[RenderPassId] {
        &[pass_ids::CLEAR]
    }

    fn execute(&mut self, ctx: &mut RenderContext) {
        // 1. Update camera uniform buffer
        // 2. Begin render pass targeting game_texture_view
        // 3. Draw fullscreen quad (6 vertices, no vertex buffer)
        // 4. Fragment shader does all the work
    }
}
```

**Pipeline insertion:** After `ClearPass`, renders directly to `game_texture`. Skip or reorder `UploadPass` since the GPU pass writes directly to the GPU texture.

**Shader resources needed:**
- `@group(0) @binding(0) var<uniform> camera: Camera` — position, direction, plane, screen size
- `@group(0) @binding(1) var<storage, read> map: array<u32>` — 2D grid data
- `@group(0) @binding(2) var wall_textures: texture_2d_array<f32>` — packed wall textures
- `@group(0) @binding(3) var wall_sampler: sampler`

**Pros:**
- Massively parallel — every pixel computed simultaneously on GPU
- Natural fit with the existing wgpu-based renderer
- Can leverage existing `fullscreen_quad.wgsl` vertex shader pattern

**Cons:**
- More complex setup (pipelines, bind groups, buffer management)
- Harder to debug (GPU shader debugging is limited)
- DDA loops in shaders can have divergent branch performance

#### Option C — Hybrid (CPU Raycasting + GPU Compositing)

CPU performs DDA and writes to `pixel_buffer`. GPU handles post-processing via existing passes.

- CPU: DDA loop, wall/floor rendering, sprite compositing → `pixel_buffer`
- `UploadPass`: transfers to GPU
- GPU: existing `LightingPass`, `EffectsPass` apply fog, lighting, CRT effects, bloom, etc.

**Pros:** Best of both worlds — easy CPU raycasting with GPU post-processing.

**Cons:** Still CPU-bound for the raycasting step.

### 5.3 Recommended Approach

**Start with Option C (hybrid)**, then migrate to Option B if performance requires it.

Rationale:
1. CPU raycasting is straightforward to implement and debug
2. Ryo2D's existing pipeline already handles CPU→GPU upload and post-processing
3. The `raycaster` crate proves CPU raycasting at 1280x800 is viable (~2-3ms with rayon)
4. GPU effects (lighting, bloom, CRT, fog) can be applied on top for free
5. Migration path to GPU raycasting is clean — just replace the CPU pass with a GPU pass

### 5.4 ECS Components

Based on Ryo2D's `hecs`-based ECS architecture:

| Component | Data | Purpose |
|-----------|------|---------|
| `RaycastMap` | 2D grid of tile IDs, tile size, wall textures | World grid (resource/singleton) |
| `RaycastCamera` | position, direction, plane (FOV), height | Viewpoint (attached to player entity) |
| `RaycastSprite` | texture_id, world_position, scale | Billboard objects |
| `RaycastLight` | position, radius, color, intensity | Point lights |
| `RaycastSector` | floor_height, ceiling_height, textures | Multi-height maps (Build-style) |
| `RaycastDoor` | state, timer, texture | Animated doors |

### 5.5 ECS Systems

| System | Queries | Purpose |
|--------|---------|---------|
| `RaycastRenderSystem` | `RaycastCamera`, `RaycastMap` (resource) | DDA raycasting, wall/floor/ceiling rendering |
| `SpriteRenderSystem` | `RaycastSprite`, `Transform` | Sort and render billboard sprites via Z-buffer |
| `LightingSystem` | `RaycastLight` | Per-tile or per-pixel lighting |
| `DoorSystem` | `RaycastDoor` | Door animation, map grid modification |
| `CameraFollowSystem` | `RaycastCamera`, `Transform` | Sync camera from player entity |

### 5.6 HUD Compositing

The standard Wolfenstein/Doom pattern:

1. Render 3D raycasted scene (walls, floors, ceilings, sprites)
2. Render 2D HUD on top (health, ammo, minimap, weapon, crosshair)

In Ryo2D, the raycasted scene occupies the early render passes. The existing `UiPass` already renders on top of everything, so HUD elements work without modification. The pipeline order naturally supports this layering:

```
ClearPass → [RaycastPass] → UploadPass → GpuSpritePass → LightingPass → UiPass → EffectsPass → PresentPass
```

---

## 6. Existing Rust Implementations

### 6.1 Published Crates

**[raycaster](https://crates.io/crates/raycaster) (by markusmoenig)**
- Full-featured Wolfenstein-style engine: textured walls, floors, ceilings, sprites, doors, fog, tile-based lighting, animation
- Multi-threaded via `rayon` (~2-3ms at 1280x800)
- Renders to `Vec<u8>` — easily composited with any GPU backend
- Single dependency: `rustc-hash` (plus `rayon`)
- Created for the [Eldiron](https://github.com/markusmoenig/Eldiron) retro RPG creator
- Internal optimization: stores image 90-degree rotated for cache-friendly column rendering

### 6.2 Notable Projects

| Project | Description | Backend |
|---------|-------------|---------|
| [Rustenstein 3D](https://github.com/AdRoll/rustenstein) | Full Wolfenstein 3D port, reads original .WL1 data files | CPU |
| [linon](https://github.com/niklaskorz/linon) | Non-linear raycasting via wgpu compute shaders with Runge-Kutta | wgpu (GPU) |
| [wolf3d.rs](https://github.com/d33kshant/wolf3d.rs) | Console/terminal-based raycasting | CPU (terminal) |
| [rust-tutorial-raycaster](https://github.com/rhian-cs/rust-tutorial-raycaster) | Follows Lode's tutorial | CPU |
| [cub3D](https://github.com/SirAlabar/cub3D) | Portals, enemies, animated weapons, interactive doors | CPU |

### 6.3 GPU Reference Implementations (Non-Rust)

| Project | Notable Features |
|---------|-----------------|
| Shadertoy: Branchless Voxel Raycasting (4dX3zl) | Branchless DDA optimized for GPU |
| Shadertoy: Branchless Voxel Ray-Casting Tex (7dK3D3) | Textured 3D voxels |
| Wolfenstein RT (WebGL, reindernijhoff.net) | Full Wolfenstein level with GPU raycasting |

---

## 7. References

### Canonical Tutorials
- [Lode's Raycasting Tutorial — Walls](https://lodev.org/cgtutor/raycasting.html) — The definitive DDA reference
- [Lode's Raycasting Tutorial — Floor/Ceiling](https://lodev.org/cgtutor/raycasting2.html) — Horizontal scanline method
- [Lode's Raycasting Tutorial — Sprites](https://lodev.org/cgtutor/raycasting3.html) — Billboard sprites with Z-buffer
- [Lode's Raycasting Tutorial — Advanced](https://lodev.org/cgtutor/raycasting4.html) — Thin walls, doors, push walls
- [Permadi's Ray Casting Tutorial](https://permadi.com/1996/05/ray-casting-tutorial-table-of-contents/) — Alternative reference with shading
- [Pikuma's Raycasting Engine Course](https://pikuma.com/courses/raycasting-engine-tutorial-algorithm-javascript) — 18-hour comprehensive course

### Mode 7 References
- [Tonc: Mode 7 Part 1](https://coranac.com/tonc/text/mode7.htm) — Perspective math derivation
- [Tonc: Mode 7 Part 2](https://www.coranac.com/tonc/text/mode7ex.htm) — Extended with pitch and horizon
- [SNESdev Wiki: Mode 7](https://snes.nesdev.org/wiki/Mode_7_transform) — Hardware-level documentation
- [Kulor's Guide to Mode 7 Perspective Planes](https://forums.nesdev.org/viewtopic.php?t=24053) — Practical implementation

### Modern GPU Approaches
- [Weekend Raytracing with wgpu](https://nelari.us/post/weekend_raytracing_with_wgpu_1/) — Fragment shader fullscreen quad in Rust
- [Learn Wgpu](https://sotrh.github.io/learn-wgpu/beginner/tutorial3-pipeline/) — Foundational wgpu pipeline
- [WebGPU Fundamentals — Compute Shaders](https://webgpufundamentals.org/webgpu/lessons/webgpu-compute-shaders.html)
- [Tour of WGSL](https://google.github.io/tour-of-wgsl/) — Interactive WGSL tutorial

### Advanced Techniques
- [Advanced Raycasting Techniques (GameDev.net)](https://archive.gamedev.net/archive/reference/articles/article872.html) — Sectors, transparent walls
- [Scott Smitelli: We Can Fix Your Raycaster](https://www.scottsmitelli.com/articles/we-can-fix-your-raycaster/) — Fisheye, projection, aspect ratio fixes
- [Rustenstein 3D Blog Post](https://tech.nextroll.com/blog/dev/2022/02/02/rustenstein.html) — Detailed Wolfenstein-to-Rust port write-up

### Books
- **Game Engine Black Book: Wolfenstein 3D** by Fabien Sanglard — Deep dive into original engine internals
