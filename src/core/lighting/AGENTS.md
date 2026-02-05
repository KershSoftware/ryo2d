# AGENTS.md - 2D Lighting System

## Overview

This folder contains the complete 2D lighting implementation for Ryo2D. The system provides dynamic lighting with point lights, spotlights, ambient illumination, and shadow casting.

## Architecture

The lighting system uses a **deferred lighting approach**:

1. **Light Accumulation Pass** - All lights render to a light buffer using additive blending
2. **Shadow Pass** - Light occluders cast shadows by projecting geometry away from lights
3. **Composition Pass** - Final scene = base scene × (ambient + light buffer)

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Public exports and module documentation |
| `light.rs` | `Light2D` component (point lights, spotlights) |
| `occluder.rs` | `LightOccluder2D` component for shadow casting |
| `ambient.rs` | `AmbientLight2D` global illumination settings |
| `renderer.rs` | GPU rendering implementation for lights and shadows |

## Components

### Light2D

Two light types:

```rust
// Point light - omnidirectional
Light2D::Point {
    color: Color,
    radius: f32,
    intensity: f32,
    falloff: f32,
    cast_shadows: bool,
}

// Spotlight - directional cone
Light2D::Spot {
    color: Color,
    radius: f32,
    intensity: f32,
    falloff: f32,
    angle: f32,          // cone angle in degrees
    direction: f32,      // 0° = right, 90° = down
    softness: f32,       // edge softness (0.0-1.0)
    cast_shadows: bool,
}
```

**Convenience constructors:**
- `Light2D::torch(radius)` - Warm orange point light
- `Light2D::flashlight(radius, direction)` - White spotlight
- `Light2D::candle(radius)` - Flickering warm light (managed by user)
- `Light2D::point(color, radius)` - Custom point light
- `Light2D::spot(color, radius, angle, direction)` - Custom spotlight

### LightOccluder2D

Blocks light and casts shadows:

```rust
LightOccluder2D {
    vertices: Vec<Vec2>,  // polygon vertices (CCW winding)
    enabled: bool,
}
```

**Convenience constructors:**
- `LightOccluder2D::rect(width, height)` - Rectangular occluder
- `LightOccluder2D::circle(radius, segments)` - Circular occluder
- `LightOccluder2D::polygon(vertices)` - Custom polygon

### AmbientLight2D

Global base illumination:

```rust
AmbientLight2D {
    color: Color,
    intensity: f32,
}
```

Default ambient is very dark to emphasize dynamic lights. Insert as a singleton resource.

## Usage Patterns

### Basic Lighting Setup

```rust
fn init(&mut self, ctx: &mut GameContext) {
    // Set ambient light
    ctx.world.insert_resource(AmbientLight2D::new(
        Color::new(20, 20, 30, 255),
        0.2,
    ));
    
    // Add a torch
    ctx.world.spawn((
        Transform::at(400.0, 300.0),
        Light2D::torch(150.0),
    ));
    
    // Add wall occluders
    ctx.world.spawn((
        Transform::at(200.0, 400.0),
        LightOccluder2D::rect(100.0, 20.0),
    ));
}
```

### Moving Lights

Lights follow their entity's `Transform`:

```rust
// Update the light position by updating the transform
for (entity, (transform, light)) in ctx.world.query_mut::<(&mut Transform, &Light2D)>() {
    transform.position.x += velocity.x * dt;
    transform.position.y += velocity.y * dt;
}
```

### Dynamic Light Effects

Modify light properties at runtime:

```rust
// Flickering candle effect
if let Ok(mut light) = ctx.world.get_mut::<Light2D>(candle_entity) {
    if let Light2D::Point { intensity, .. } = light.as_mut() {
        *intensity = 0.8 + 0.2 * (time * 10.0).sin();
    }
}

// Rotating spotlight
if let Ok(mut light) = ctx.world.get_mut::<Light2D>(spotlight_entity) {
    if let Light2D::Spot { direction, .. } = light.as_mut() {
        *direction = (*direction + 90.0 * dt) % 360.0;
    }
}
```

## Rendering Implementation

The renderer (`renderer.rs`) handles:

1. **Light Pass** - Each light renders a quad with a custom shader:
   - Point lights: radial gradient with falloff
   - Spotlights: cone-shaped gradient with soft edges
   - Additive blending accumulates light contributions

2. **Shadow Pass** - For shadow-casting lights:
   - Project occluder vertices away from light position
   - Render shadow geometry to stencil/alpha mask
   - Subtract shadows from light buffer

3. **Composition** - Multiply final scene by lighting:
   ```
   final_color = base_color × (ambient + light_buffer)
   ```

## Performance Considerations

- **Light count** - Each light is a separate draw call; keep under 20-30 for best performance
- **Occluder complexity** - Simple rectangles/circles are fastest; complex polygons cost more
- **Shadow casting** - Disable shadows on distant/unimportant lights
- **Light culling** - Lights outside the viewport are automatically skipped
- **Radius optimization** - Smaller radius = faster rendering (fewer pixels affected)

## Design Philosophy

**Simple but Flexible** - Common use cases (torch, flashlight) have one-line constructors, but all parameters are exposed for customization.

**ECS Integration** - Lights are components, not special objects. They compose naturally with Transform, Velocity, etc.

**No Global State** - Each frame, the lighting system queries for Light2D and LightOccluder2D components and renders them. No persistent light manager.

**Artist-Friendly** - Colors use 0-255 RGB, not 0.0-1.0. Radius in pixels, not abstract units.

## Shadow System

The lighting system supports multiple shadow types:

### Shadow Volumes (Hard Shadows)
```rust
Light2D::point(color, radius).with_shadows()
```
Projects occluder geometry away from lights using GPU-rendered shadow volumes.

### Soft Shadows
```rust
Light2D::point(color, radius).with_soft_shadows(0.5)
```
Adds penumbra/blur to shadow edges for more natural lighting.

### Colored Shadows
```rust
Light2D::point(color, radius).with_shadows().with_shadow_color(Color::new(50, 20, 80, 255))
```
Tint shadows with color for atmospheric effects (stained glass, sunset, etc.).

### Ray-marched Shadows
```rust
Light2D::point(color, radius).with_raymarched_shadows(32)
```
Per-pixel ray casting for accurate shadows. Sample count (4-64) trades quality vs performance.

### Sprite-Based Shadows

**Drop Shadows** - Artist-controlled offset shadows:
```rust
Renderable::sprite(handle).with_drop_shadow(DropShadow::below(8.0))
```

**Blob Shadows** - Elliptical grounding shadows:
```rust
Renderable::sprite(handle).with_blob_shadow(BlobShadow::ellipse(20.0, 10.0))
```

**Contact Shadows** - Darkening at object bases:
```rust
Renderable::sprite(handle).with_contact_shadow(ContactShadow::new().radius(8.0))
```

### Ambient Occlusion (Post-Process)
```rust
ctx.effects.set_ambient_occlusion(0.3, 16.0);
```
Screen-space corner/crevice darkening as a post-process effect.

## Future Work

- [x] Volumetric lights (god rays) ✅ Implemented
- [x] Atmospheric scattering ✅ Implemented
- [x] Volumetric spotlight beams ✅ Implemented
- [x] **Shadow System** ✅ Complete
  - [x] Hard shadows (shadow volumes)
  - [x] Drop shadows (sprite-based)
  - [x] Blob shadows (character grounding)
  - [x] Soft shadows (penumbra)
  - [x] Colored shadows
  - [x] Contact shadows
  - [x] Ambient occlusion (post-process)
  - [x] Ray-marched shadows (per-pixel)
- [ ] Light cookies (textured lights)
- [ ] Normal maps for depth-aware lighting
- [ ] Light layers (selective illumination)

## Notes for AI Agents

When working on lighting:

1. **Test with examples** - The lighting system is visual; run examples to validate changes
2. **Shader knowledge** - `renderer.rs` uses WGSL shaders; understand vertex/fragment stages
3. **Performance critical** - Lighting runs every frame; profile before optimizing
4. **Color consistency** - Use `Color` type from `ui` module, not raw RGB tuples
5. **Transform dependency** - Lights require a `Transform` component to position them
6. **Coordinate space** - Light positions are in world space; occluder vertices are local space

When adding new features:
- Add convenience constructors to `Light2D`/`LightOccluder2D`
- Update rustdoc examples in the module files
- Add test cases in examples/demos/lighting
- Document performance implications
