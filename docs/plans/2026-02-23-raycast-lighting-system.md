# Raycaster Professional Lighting System — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a professional lighting pipeline for the raycasting renderer, modeled on Doom/GZDoom/Prodeus: smooth ambient lightmaps, camera-distance light diminishing, 3D light positions, configurable falloff curves, and sprite lighting integration.

**Architecture:** Six-layer lighting model applied per-pixel in the fragment shader: (1) smooth lightmap ambient via bilinear-sampled texture, (2) global ambient color/intensity, (3) wall-side directional bias (existing), (4) camera-distance light diminishing, (5) dynamic point lights with 3D height and configurable falloff, (6) distance fog (existing). Sprites share the dynamic light buffer for environmental consistency.

**Tech Stack:** Rust (wgpu), WGSL shaders, bytemuck for GPU struct packing

**Key files overview:**
- `src/core/raycast/systems.rs` — GPU uniform structs (CameraUniform, MapInfoUniform, LightInstance)
- `src/core/raycast/gpu.rs` — GpuRaycastRenderer: buffers, bind groups, pipelines
- `src/core/raycast/components.rs` — ECS components (RaycastLight, RaycastCamera)
- `src/core/raycast/world.rs` — MapCell, RaycastWorld, GPU packing
- `src/core/shaders/core/raycast.wgsl` — Wall/floor/ceiling raycasting shader
- `src/core/shaders/core/raycast_sprite.wgsl` — Billboard sprite shader
- `src/core/engine.rs` — Sync ECS → GPU each frame (~lines 2243-2339)
- `examples/demos/raycasting/main.rs` — Demo showcasing the lighting

---

## Task 1: Expand LightInstance with height and falloff

Add `height` and `falloff` fields to the light GPU struct so the shader can compute proper 3D distance and use configurable attenuation curves. Repurpose existing `_pad` field and expand struct from 32 to 48 bytes.

**Files:**
- Modify: `src/core/raycast/systems.rs` (LightInstance struct + size test)
- Modify: `src/core/raycast/components.rs` (RaycastLight component)
- Modify: `src/core/raycast/gpu.rs` (light buffer size: MAX_LIGHTS * 48)
- Modify: `src/core/engine.rs` (~line 2301-2317, light sync)
- Modify: `src/core/shaders/core/raycast.wgsl` (LightInstance struct + apply_lights)

**Step 1: Update LightInstance struct**

In `src/core/raycast/systems.rs`, expand LightInstance:

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightInstance {
  pub pos: [f32; 2],       // 8 bytes  (offset 0)
  pub radius: f32,         // 4 bytes  (offset 8)
  pub intensity: f32,      // 4 bytes  (offset 12)
  pub color: [f32; 3],     // 12 bytes (offset 16)
  pub height: f32,         // 4 bytes  (offset 28)  — NEW: world-space height (0.5 = mid-wall)
  pub falloff: f32,        // 4 bytes  (offset 32)  — NEW: 1.0=linear, 2.0=quadratic, 3.0=cubic
  pub _pad: [f32; 3],      // 12 bytes (offset 36)  — pad to 48 bytes
}
```

Update the size test to `assert_eq!(std::mem::size_of::<LightInstance>(), 48);`

**Step 2: Update RaycastLight component**

In `src/core/raycast/components.rs`, add fields:

```rust
pub struct RaycastLight {
  pub radius: f32,
  pub color: (f32, f32, f32),
  pub intensity: f32,
  pub flicker: bool,
  pub height: f32,    // NEW — default 0.5 (mid-wall)
  pub falloff: f32,   // NEW — default 2.0 (quadratic)
}
```

Update `RaycastLight::new()` to set `height: 0.5, falloff: 2.0`.

**Step 3: Update light buffer size in gpu.rs**

Change the light buffer allocation from `MAX_LIGHTS * 32` to `MAX_LIGHTS * 48`:

```rust
let light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
  label: Some("Raycast Light Buffer"),
  size: (MAX_LIGHTS * std::mem::size_of::<LightInstance>()) as u64,
  // ...
});
```

**Step 4: Update engine.rs light sync**

In the light sync loop (~line 2301-2317), pass the new fields:

```rust
ray_lights.push(LightInstance {
  pos: [transform.x, transform.y],
  radius: light.radius,
  intensity: light.intensity,
  color: [light.color.0, light.color.1, light.color.2],
  height: light.height,
  falloff: light.falloff,
  _pad: [0.0; 3],
});
```

**Step 5: Update shader LightInstance struct and apply_lights**

In `raycast.wgsl`, update the struct:

```wgsl
struct LightInstance {
    pos: vec2<f32>,
    radius: f32,
    intensity: f32,
    color: vec3<f32>,
    height: f32,
    falloff: f32,
    _pad: vec3<f32>,
}
```

In `apply_lights`, replace hardcoded `0.5` with `light.height`:

```wgsl
// Before:
let dist = sqrt(dist_2d * dist_2d + vert_offset * vert_offset);
// After:
let vert_diff = vert_offset - (light.height - 0.5);
let dist = sqrt(dist_2d * dist_2d + vert_diff * vert_diff);
```

Replace hardcoded smoothstep with configurable falloff:

```wgsl
// Before:
let atten = light.intensity * (1.0 - smoothstep(0.0, 1.0, norm));
// After:
let atten = light.intensity * pow(max(1.0 - norm, 0.0), light.falloff);
```

**Step 6: Run tests and verify**

Run: `cargo test --lib -p ryo2d -- raycast`
Expected: All pass (struct size test updated to 48)

Run: `cargo build --workspace`
Expected: Clean build

**Step 7: Commit**

```
feat(raycasting): add light height and configurable falloff to RaycastLight
```

---

## Task 2: Expand MapInfoUniform with ambient and diminishing

Add global ambient light and camera-distance diminishing parameters to the map info uniform so the shader can apply Doom-style atmospheric lighting.

**Files:**
- Modify: `src/core/raycast/systems.rs` (MapInfoUniform struct + size test)
- Modify: `src/core/shaders/core/raycast.wgsl` (MapInfo struct + diminishing logic)
- Modify: `examples/demos/raycasting/main.rs` (set ambient/diminishing values)

**Step 1: Expand MapInfoUniform**

In `src/core/raycast/systems.rs`:

```rust
pub struct MapInfoUniform {
  pub dimensions: [u32; 2],      // 8 bytes  (offset 0)
  pub max_distance: f32,         // 4 bytes  (offset 8)
  pub fog_density: f32,          // 4 bytes  (offset 12)
  pub fog_color: [f32; 3],       // 12 bytes (offset 16)
  pub diminish_factor: f32,      // 4 bytes  (offset 28) — NEW: 0.0=none, 0.1=subtle, 0.3=strong
  pub ambient_color: [f32; 3],   // 12 bytes (offset 32) — NEW: global ambient RGB
  pub ambient_intensity: f32,    // 4 bytes  (offset 44) — NEW: 0.0..1.0
}
// Total: 48 bytes
```

Update size test to `assert_eq!(std::mem::size_of::<MapInfoUniform>(), 48);`

**Step 2: Update shader MapInfo struct**

In `raycast.wgsl`:

```wgsl
struct MapInfo {
    dimensions: vec2<u32>,
    max_distance: f32,
    fog_density: f32,
    fog_color: vec3<f32>,
    diminish_factor: f32,
    ambient_color: vec3<f32>,
    ambient_intensity: f32,
}
```

**Step 3: Add light diminishing to shader**

After lighting is applied but before fog, add camera-distance darkening:

```wgsl
// Light diminishing: surfaces darken with distance from camera (Doom-style)
if map_info.diminish_factor > 0.0 {
    let diminish = 1.0 / (1.0 + perp_wall_dist * map_info.diminish_factor);
    final_color = final_color * diminish;
}
```

Apply the same diminishing in the floor and ceiling code paths using `floor_dist` and `ceil_dist` respectively.

**Step 4: Apply global ambient in shader**

Before per-tile light_level is applied, blend in the global ambient:

```wgsl
let global_ambient = map_info.ambient_color * map_info.ambient_intensity;
// Ensure surfaces are never darker than global ambient
wall_color = max(wall_color, tex_color.rgb * global_ambient);
```

**Step 5: Update demo**

In `examples/demos/raycasting/main.rs`:

```rust
rc.set_map_info(MapInfoUniform {
  dimensions: [32, 32],
  max_distance: 32.0,
  fog_density: 0.04,
  fog_color: [0.05, 0.05, 0.1],
  diminish_factor: 0.08,
  ambient_color: [0.6, 0.65, 0.8],
  ambient_intensity: 0.05,
});
```

**Step 6: Run tests and verify**

Run: `cargo test --lib -p ryo2d -- raycast`
Run: `cargo build --workspace`

**Step 7: Commit**

```
feat(raycasting): add global ambient light and camera-distance diminishing
```

---

## Task 3: Smooth lightmap texture

Replace the per-tile step-function `light_level` with a bilinear-sampled lightmap texture. This is the biggest visual improvement — light zones transition smoothly across tile boundaries instead of having hard edges.

**Files:**
- Modify: `src/core/raycast/gpu.rs` (lightmap texture, linear sampler, bind group entries)
- Modify: `src/core/raycast/world.rs` (generate lightmap data)
- Modify: `src/core/shaders/core/raycast.wgsl` (sample lightmap instead of cell light_level)

**Step 1: Add lightmap generation to RaycastWorld**

In `src/core/raycast/world.rs`, add a method to generate lightmap data:

```rust
/// Generate a lightmap texture from per-tile light_level values.
/// Returns a Vec<f32> of width × height values suitable for R32Float texture upload.
pub fn generate_lightmap(&self) -> Vec<f32> {
    let mut data = vec![1.0_f32; self.width * self.height];
    for y in 0..self.height {
        for x in 0..self.width {
            data[y * self.width + x] = self.cells[y * self.width + x].light_level;
        }
    }
    data
}
```

**Step 2: Add lightmap texture and linear sampler to GpuRaycastRenderer**

In `src/core/raycast/gpu.rs`, add fields:

```rust
lightmap_texture: wgpu::Texture,
lightmap_texture_view: wgpu::TextureView,
lightmap_sampler: wgpu::Sampler,
```

Create them in `new()`:
- Texture: `R32Float`, dimensions matching map (e.g., 256x256 max), `TEXTURE_BINDING | COPY_DST`
- Sampler: `mag_filter: Linear`, `min_filter: Linear`, `address_mode: ClampToEdge`

**Step 3: Add bind group layout entries**

Add two new entries to `wall_bind_group_layout`:

```rust
// Binding 8: lightmap texture (R32Float, filterable)
wgpu::BindGroupLayoutEntry {
    binding: 8,
    visibility: wgpu::ShaderStages::FRAGMENT,
    ty: wgpu::BindingType::Texture {
        multisampled: false,
        view_dimension: wgpu::TextureViewDimension::D2,
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
    },
    count: None,
},
// Binding 9: lightmap sampler (linear filtering)
wgpu::BindGroupLayoutEntry {
    binding: 9,
    visibility: wgpu::ShaderStages::FRAGMENT,
    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
    count: None,
},
```

Add corresponding entries to the bind group creation in `flush_pending()`.

**Step 4: Upload lightmap when map is dirty**

In `flush_pending()` or `update_map()`, when the world is dirty, also generate and upload the lightmap:

```rust
let lightmap_data = world.generate_lightmap();
queue.write_texture(
    wgpu::TexelCopyTextureInfo {
        texture: &self.lightmap_texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
    },
    bytemuck::cast_slice(&lightmap_data),
    wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(world.width() as u32 * 4),
        rows_per_image: None,
    },
    wgpu::Extent3d {
        width: world.width() as u32,
        height: world.height() as u32,
        depth_or_array_layers: 1,
    },
);
```

**Step 5: Update shader to sample lightmap**

In `raycast.wgsl`, add bindings:

```wgsl
@group(0) @binding(8) var lightmap: texture_2d<f32>;
@group(0) @binding(9) var lightmap_sampler: sampler;
```

Replace per-tile light_level lookups with lightmap samples. For walls:

```wgsl
// Before:
let cell_light = get_light_level(cell);
wall_color = wall_color * cell_light;

// After:
let lm_uv = world_hit / vec2<f32>(f32(map_info.dimensions.x), f32(map_info.dimensions.y));
let cell_light = textureSample(lightmap, lightmap_sampler, lm_uv).r;
wall_color = wall_color * cell_light;
```

For floor pixels, use `floor_world` for UV. For ceiling pixels, use `ceil_world`.

Note: `get_light_level()` is still used by `apply_lights` for the `ambient` parameter. Replace those too with the lightmap sample, or pass the sampled value through.

**Step 6: Test and verify**

Run: `cargo test --lib -p ryo2d -- raycast`
Run: `cargo build --workspace`
Visual: Run raycasting demo — dark room boundaries should now blend smoothly into the lit area.

**Step 7: Commit**

```
feat(raycasting): add bilinear lightmap texture for smooth ambient transitions
```

---

## Task 4: Sprite lighting integration

Sprites currently use a simple distance-darkening formula (`1.0 / (1.0 + depth * 0.15)`) that ignores dynamic lights entirely. Add the light buffer to the sprite shader so sprites are lit consistently with their environment.

**Files:**
- Modify: `src/core/raycast/gpu.rs` (sprite bind group layout + bind group creation)
- Modify: `src/core/shaders/core/raycast_sprite.wgsl` (add light bindings + apply lighting)

**Step 1: Expand sprite bind group layout**

In `gpu.rs`, add light buffer, light count, and lightmap bindings to the sprite bind group layout:

```rust
// Binding 5: light buffer (storage, read-only)
// Binding 6: light count (uniform)
// Binding 7: lightmap texture
// Binding 8: lightmap sampler
```

Update the bind group creation in `flush_pending()` to include these buffers.

**Step 2: Add lighting to sprite fragment shader**

In `raycast_sprite.wgsl`, add the light structs and bindings (same as wall shader). Then in the fragment shader, replace the distance darkening with proper lighting:

```wgsl
// Look up the sprite's world position for lighting
let sprite = sprites[in.texture_id]; // Need to pass sprite index
let world_pos = sprite.pos;

// Sample lightmap for ambient
let lm_uv = world_pos / vec2<f32>(f32(map_dims.x), f32(map_dims.y));
let ambient = textureSample(lightmap, lightmap_sampler, lm_uv).r;

// Apply dynamic lights (at sprite height, no vertical offset)
var light_contrib = vec3<f32>(0.0);
for (var i: u32 = 0u; i < light_count; i++) {
    let light = lights[i];
    let dist = distance(world_pos, light.pos);
    if dist < light.radius {
        let norm = dist / light.radius;
        let atten = light.intensity * pow(max(1.0 - norm, 0.0), light.falloff);
        light_contrib += light.color * atten;
    }
}

let surface = color.rgb / max(ambient, 0.05);
let lit_color = min(color.rgb * ambient + surface * light_contrib, vec3(1.0));

// Apply diminishing
let dim = 1.0 / (1.0 + in.sprite_depth * diminish_factor);
return vec4<f32>(lit_color * dim, 1.0);
```

Note: sprite shadow rays are optional (expensive for sprites, skip for now — sprites are small and shadow precision matters less).

**Step 3: Pass sprite instance index to fragment shader**

The fragment shader needs to know which sprite instance it's rendering to look up world position. Pass the `instance_index` as a flat interpolated value from vertex to fragment shader.

**Step 4: Test and verify**

Run: `cargo build --workspace`
Visual: Sprites near torches should now glow warm/cool instead of uniformly dim.

**Step 5: Commit**

```
feat(raycasting): integrate dynamic lighting with sprite renderer
```

---

## Task 5: Polish and demo update

Update the raycasting demo to showcase all the new lighting features and tune the values for a professional look.

**Files:**
- Modify: `examples/demos/raycasting/main.rs`

**Step 1: Add varied lighting zones**

Create areas with different ambient levels that demonstrate the smooth lightmap transitions:
- Brightly lit main hall (light_level = 1.0)
- Dim corridor connecting rooms (light_level = 0.4)
- Dark room with torches (light_level = 0.15, existing)
- Gradual transition zones

**Step 2: Add more dynamic lights**

- Add a light near the doorway that spills into the corridor
- Use different falloff values to show the difference (quadratic vs cubic)
- Place a barrel near a light to demonstrate sprite lighting

**Step 3: Tune diminishing and ambient**

Experiment with values:
- `diminish_factor: 0.06..0.12` — subtle depth without losing detail
- `ambient_intensity: 0.03..0.08` — prevents pure black without flattening contrast
- `ambient_color` — slight blue tint for outdoor/cave feel

**Step 4: Test and verify**

Visual: Walk through the entire demo verifying:
- Smooth transitions between light zones
- No hard edges at tile boundaries
- Sprites lit consistently with environment
- Distant surfaces darken naturally
- Torch corners look atmospheric

**Step 5: Commit**

```
feat(raycasting): update demo with professional lighting showcase
```

---

## Future Work (Phase 3)

These features are not in scope for this plan but are natural next steps:

- **Spot lights** — Cone lights with direction, angle, and soft edges. Requires expanding LightInstance with direction/angle fields and adding cone test to shader.
- **Soft shadows** — Multi-sample shadow rays with slight offsets for penumbra. Expensive (~4x shadow cost) but eliminates hard shadow edges.
- **Colored shadows** — Shadow rays contribute a shadow color instead of binary block.
- **Light flicker system** — Per-light flicker patterns (sine, random, strobe) driven by `camera.time` in the shader. RaycastLight already has a `flicker: bool` field.
- **Emissive surfaces** — MapCell flag for self-illuminating tiles that contribute to the lightmap.
- **Higher-res lightmap** — 2x or 4x map resolution lightmap with light propagation/bleeding for even smoother ambient zones.
