# AGENTS.md - Shader System

This module provides the post-processing effect pipeline for Ryo2D.

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Public exports and shader embedding via `include_str!` |
| `effects.rs` | `PostProcessEffect` enum and `EffectParams` uniform struct |
| `manager.rs` | `EffectsManager` for effect stacks, timing, and convenience API |
| `pipeline.rs` | `PostProcessPipeline` GPU resources and ping-pong buffer logic |

### Shader Directories

| Directory | Purpose |
|-----------|---------|
| `core/` | Core rendering shaders (sprites, lights, shadows) |
| `post-process/` | Effect fragment shaders (one per effect type) |

### Core Shaders

| File | Purpose |
|------|---------|
| `light.wgsl` | Point light and spotlight rendering |
| `shadow.wgsl` | Shadow volume geometry rendering (hard/soft/colored shadows) |
| `raymarch_shadow.wgsl` | Per-pixel ray-marched shadows |
| `raycast.wgsl` | DDA wall raycasting — textured walls, floors, ceilings, doors, dynamic lights, fog |
| `raycast_sprite.wgsl` | Instanced billboard sprite rendering with depth testing against wall pass |

### Shadow Shaders

**shadow.wgsl** - Renders shadow volumes with reverse-subtract blending:
- Vertex shader transforms occluder geometry projected from light
- Fragment shader applies distance-based intensity and color

**raymarch_shadow.wgsl** - Per-pixel ray casting:
- Storage buffer holds up to 64 occluder edges
- Fragment shader tests ray-edge intersections
- Configurable sample count for quality/performance tradeoff

## Architecture

```
Game Texture → [Effect 1] → [Effect 2] → ... → [Effect N] → Final Output
                  ↓            ↓                   ↓
               Tex A        Tex B               Tex A/B
                    (ping-pong buffers)
```

### Ping-Pong Pattern

The pipeline uses two intermediate textures (A and B) to chain effects:

1. First effect reads from game texture, writes to A
2. Second effect reads from A, writes to B
3. Third effect reads from B, writes to A
4. ...and so on, alternating

**Critical:** The output buffer tracking in `pipeline.rs` must correctly return whichever texture contains the final processed output.

## Key Types

### `PostProcessEffect` (effects.rs)

Enum of all supported effects with their parameters:

- `Fade { progress, color }` - Scene transitions
- `ColorTint { color, intensity }` - Color grading/mood
- `Vignette { intensity, radius, softness }` - Darkened edges
- `ScreenFlash { color, intensity }` - One-shot damage/power-up flash
- `Crt { scanline_intensity, curvature, rgb_offset }` - Retro CRT look
- `ChromaticAberration { offset, radial }` - RGB color fringing
- `GodRays { intensity, samples, decay, density, weight, threshold, light_positions }` - Radial light shafts
- `AmbientOcclusion { intensity, radius, bias }` - Corner/crevice darkening
- `Bloom { threshold, intensity, radius, blur_passes }` - Bright area glow (multi-pass)

### `EffectParams` (effects.rs)

GPU-compatible uniform struct. **Must be 64 bytes with 16-byte alignment.**

```rust
#[repr(C)]
pub struct EffectParams {
    pub color: [f32; 4],           // 16 bytes - offset 0
    pub params: [f32; 4],          // 16 bytes - offset 16
    pub time_resolution: [f32; 4], // 16 bytes - offset 32 (x=time, y=width, z=height)
    pub _padding: [f32; 4],        // 16 bytes - offset 48
}                                  // Total: 64 bytes
```

### `EffectsManager` (manager.rs)

High-level effect management:

- Maintains ordered effect stack
- Handles timed effects (auto-decay)
- Provides convenience methods (`set_fade`, `flash`, `set_vignette`, etc.)
- Implements `EffectTarget` trait for both `GameContext` and `Camera`

### `PostProcessPipeline` (pipeline.rs)

GPU resources and rendering:

- Creates/manages wgpu textures, bind groups, pipelines
- Loads and compiles WGSL shaders
- Executes effect chain with ping-pong buffers
- Returns correct output texture reference

## WGSL Shader Structure

All post-process shaders must define this uniform struct:

```wgsl
struct EffectParams {
    color: vec4<f32>,
    params: vec4<f32>,
    time_resolution: vec4<f32>,
    _padding: vec4<f32>,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(0) @binding(2) var<uniform> effect: EffectParams;

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let scene = textureSample(input_texture, input_sampler, uv);
    // Apply effect...
    return result;
}
```

**Parameter packing by effect:**

| Effect | `color` | `params` | `time_resolution` |
|--------|---------|----------|-------------------|
| Fade | fade color | x = progress | x=time, y=width, z=height |
| ColorTint | tint color | x = intensity | x=time, y=width, z=height |
| Vignette | (unused) | x=intensity, y=radius, z=softness | x=time, y=width, z=height |
| ScreenFlash | flash color | x = intensity | x=time, y=width, z=height |
| CRT | (unused) | x=scanlines, y=curvature, z=rgb_offset | x=time, y=width, z=height |
| ChromaticAberration | (unused) | x=offset, y=radial(0/1) | x=time, y=width, z=height |
| GodRays | (unused) | x=intensity, y=samples, z=decay, w=density | x=time, y=width, z=height |
| AmbientOcclusion | (unused) | x=intensity, y=radius, z=bias | x=time, y=width, z=height |
| Bloom | (unused) | x=threshold, y=intensity, z=radius, w=blur_passes | x=time, y=width, z=height |

**Note:** GodRays uses a larger 128-byte `GodRaysParams` uniform struct (not `EffectParams`). Bloom is multi-pass — see below.

### Multi-Pass Effects

**Bloom** is the first multi-pass effect. Unlike single-pass effects that run one shader per ping-pong slot, bloom runs three internal passes using a dedicated `bloom_texture` as scratch space:

```
Input in buffer X (scene)
1. Extract bright pixels:  X -> bloom_texture
2. For each blur_pass:
   a. Blur H:  bloom_texture -> Y (output buffer, used as temp)
   b. Blur V:  Y -> bloom_texture
3. Composite:  X (scene) + bloom_texture (blurred bloom) -> Y
Output in buffer Y
```

Bloom consumes one slot in the A/B ping-pong chain. Its three pipelines use different bind group layouts:
- **Extract**: standard layout (1 texture + sampler + uniform)
- **Blur**: standard layout (direction controlled by `params.w`: 0.0=H, 1.0=V)
- **Composite**: custom layout (2 textures + sampler + uniform) via `bloom_composite_bind_group_layout`

## Adding a New Effect

1. **Add variant to `PostProcessEffect`** in `effects.rs`:
   ```rust
   pub enum PostProcessEffect {
       // ...existing...
       NewEffect { param1: f32, param2: f32 },
   }
   ```

2. **Add constructor** in `impl PostProcessEffect`:
   ```rust
   pub fn new_effect(param1: f32, param2: f32) -> Self {
       Self::NewEffect { param1, param2 }
   }
   ```

3. **Update `EffectParams::from_effect`** to pack parameters:
   ```rust
   PostProcessEffect::NewEffect { param1, param2 } => EffectParams {
       color: [0.0; 4],
       params: [param1, param2, 0.0, 0.0],
       time_resolution: [time, resolution.0 as f32, resolution.1 as f32, 0.0],
       _padding: [0.0; 4],
   },
   ```

4. **Create shader** at `post-process/new_effect.wgsl`

5. **Register shader** in `mod.rs`:
   ```rust
   pub mod post_process_shaders {
       // ...existing...
       pub const NEW_EFFECT: &str = include_str!("post-process/new_effect.wgsl");
   }
   ```

6. **Add to pipeline** in `pipeline.rs`:
   - Add to shader loading in `new()`
   - Add match arm in `get_pipeline_for_effect()`

7. **Add category** in `manager.rs`:
   - Add `NewEffect` to `EffectCategory` enum
   - Add match arm in `EffectCategory::from_effect()`

8. **Add convenience method** in `EffectsManager`:
   ```rust
   pub fn set_new_effect(&mut self, param1: f32, param2: f32) {
       let effect = PostProcessEffect::new_effect(param1, param2);
       self.set_effect(effect);
   }
   ```

## Common Issues

### Buffer Alignment Errors

WGSL requires 16-byte alignment for struct members. Arrays like `array<f32, N>` have stride 4 which fails validation. Always use `vec4<f32>` for padding.

### Effects Not Visible

Check ping-pong output buffer logic in `pipeline.rs`. The returned texture must be whichever buffer contains the final processed output, not the input to the last effect.

### Texture Usage Flags

Post-process textures need `COPY_SRC | COPY_DST | TEXTURE_BINDING | RENDER_ATTACHMENT` if copying between them.
