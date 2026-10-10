---
name: ryo-lighting
description: 2D lighting system - lights, shadows, ambient occlusion, volumetric effects.
metadata:
  version: 1.0.0
---

# Ryo2D Lighting Specialist

You help developers implement lighting and shadow effects.

## Resources

- **Lighting**: `src/core/lighting/`
- **Shadow Shaders**: `src/core/shaders/core/shadow.wgsl`, `raymarch_shadow.wgsl`
- **AO Shader**: `src/core/shaders/post-process/ambient_occlusion.wgsl`
- **Demo**: `examples/demos/shadows/`

## Light Types

### Point Light
```rust
Light2D::point(Color::WHITE, 200.0)
    .with_intensity(1.3)
    .with_falloff(2.0)

// Presets
Light2D::torch(150.0)       // Warm orange
Light2D::moonlight(150.0)   // Cool blue
Light2D::lamp(150.0)        // Bright white
Light2D::warning(100.0)     // Red
Light2D::magic(120.0, color) // Custom color, soft falloff
```

### Spotlight
```rust
Light2D::spot(Color::WHITE, 200.0, 45.0, 270.0)  // color, radius, cone angle (degrees), direction (degrees)
    .with_intensity(1.5)
    .with_soft_shadows(0.3)

// Preset
Light2D::flashlight(200.0, 270.0)
```

## Shadow Types

### Hard Shadows (Shadow Volumes)
```rust
Light2D::point(color, radius).with_shadows()
```
Projects occluder geometry away from light.

### Soft Shadows
```rust
Light2D::point(color, radius).with_soft_shadows(0.5)  // softness 0.0-1.0
```

### Colored Shadows
```rust
Light2D::point(color, radius)
    .with_shadows()
    .with_shadow_color(Color::new(50, 30, 80, 255))
```

### Ray-marched Shadows
```rust
Light2D::point(color, radius).with_raymarched_shadows(32)  // samples 4-64
```
Per-pixel ray casting. Higher samples = better quality, slower.

## Sprite Shadows

### Drop Shadow
```rust
Renderable::sprite(h).with_drop_shadow(DropShadow::below(8.0))
Renderable::sprite(h).with_drop_shadow(DropShadow::diagonal(6.0))
Renderable::sprite(h).with_drop_shadow(DropShadow::squashed(12.0, 0.3))

// Custom
DropShadow::new().offset(4.0, 8.0).color(Color::new(0, 0, 0, 100)).scale(1.0, 0.6)
```

### Blob Shadow
```rust
Renderable::sprite(h).with_blob_shadow(
    BlobShadow::ellipse(20.0, 10.0)
        .offset(48.0)
        .with_height_scale(true)
        .max_height(150.0)
)
```

### Contact Shadow
```rust
Renderable::sprite(h).with_contact_shadow(
    ContactShadow::new().radius(8.0).intensity(0.6).falloff(2.0)
)
```

## Ambient Occlusion

Screen-space post-process:

```rust
ctx.effects.set_ambient_occlusion(0.3, 16.0);  // intensity, radius
ctx.effects.set_ambient_occlusion_custom(0.3, 16.0, 0.05);  // + bias
ctx.effects.disable_ambient_occlusion();
```

## Light Occluders

```rust
LightOccluder2D::rect(width, height)
LightOccluder2D::circle(radius, segments)
LightOccluder2D::polygon(vertices)  // CCW winding
```

## Ambient Light

```rust
// `lighting` is `ctx.lighting.as_mut()` (None when lighting is disabled)
lighting.set_ambient(AmbientLight::new(Color::new(20, 20, 40, 255), 0.15));

// Presets
AmbientLight::dark()     // 0 intensity
AmbientLight::night()    // Dim blue
AmbientLight::moonlit()  // Blue tint
AmbientLight::indoor()   // Warm dim
AmbientLight::daylight() // Bright
AmbientLight::sunset()   // Warm orange
```

## Volumetric Effects

```rust
Light2D::point(color, radius)
    .with_volumetric_settings(VolumetricSettings::new(0.3, 0.5, 0.2))  // intensity, density, scattering
    .with_god_rays()

// Or defaults: .with_volumetric() (beam), .with_volumetric_and_god_rays()

lighting.set_atmosphere(
    Atmosphere::new(0.3, Color::new(100, 120, 150, 255))  // density, fog color
        .with_scattering(0.2)
        .with_god_rays(),
);
// Presets: Atmosphere::hazy() / dusty() / foggy() / smoky() / underwater()
```

## Architecture

```
1. Light Accumulation Pass - Lights render to buffer (additive)
2. Shadow Pass - Occluders subtract from buffer (reverse-subtract)
3. Composition Pass - final = scene × (ambient + light_buffer)
```

## Performance

- Keep lights under 20-30
- Disable shadows on unimportant lights
- Simple occluder shapes are faster
- Ray-march: use 16-32 samples, 64 only when needed
- AO: lower radius = faster

## Tips

- Lights need `Transform` component for position
- Trails and lines render before lighting, so lighting (and shadows) apply to them. `.with_lighting(false)` on a `Trail2D`/`Line2D` draws it at full brightness on top of the lit scene instead (glowing lasers, magic); post-processing still applies
- Occluder vertices are local space, transformed by entity's Transform
- Shadow intensity multiplied by 10x to ensure full light subtraction
- Ambient light fills shadowed areas - set to 0 for pitch black shadows
