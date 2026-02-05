# Lighting System

Ryo2D provides a complete 2D lighting system with dynamic lights, shadows, and atmospheric effects.

## Basic Setup

Enable lighting in your scene:

```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Configure ambient light
    if let Some(lighting) = ctx.lighting {
        lighting.set_ambient(AmbientLight::new(Color::new(20, 20, 30, 255), 0.1));
    }

    // Add a point light
    ctx.world.spawn((
        Transform::new(400.0, 300.0),
        Light2D::point(Color::WHITE, 200.0),
    ));
}
```

## Light Types

### Point Light

Omnidirectional light that radiates in all directions:

```rust
// Basic point light
Light2D::point(Color::WHITE, 150.0)

// With intensity
Light2D::point(Color::WHITE, 150.0).with_intensity(1.5)

// Convenience presets
Light2D::torch(150.0)  // Warm orange
Light2D::candle(80.0)  // Flickering warm (update manually)
```

### Spotlight

Directional cone of light:

```rust
// angle = cone angle in degrees, direction = rotation (0° = right)
Light2D::spot(Color::WHITE, 200.0, 45.0, 270.0)

// Convenience preset
Light2D::flashlight(200.0, 270.0)  // White spotlight pointing up
```

## Shadow Types

### Hard Shadows (Shadow Volumes)

Sharp, binary shadows from occluder geometry:

```rust
// Light with shadows
Light2D::point(Color::WHITE, 200.0).with_shadows()

// Add occluder to cast shadows
ctx.world.spawn((
    Transform::new(300.0, 300.0),
    LightOccluder2D::rect(50.0, 50.0),
));
```

### Soft Shadows

Shadows with penumbra (gradual edge falloff):

```rust
Light2D::point(Color::WHITE, 200.0).with_soft_shadows(0.5)
```

The softness parameter (0.0-1.0) controls edge blur.

### Colored Shadows

Tint shadows for atmospheric effects:

```rust
Light2D::point(Color::WHITE, 200.0)
    .with_shadows()
    .with_shadow_color(Color::new(50, 30, 80, 255))
```

### Ray-marched Shadows

Per-pixel ray casting for accurate shadows on complex shapes:

```rust
Light2D::point(Color::WHITE, 200.0).with_raymarched_shadows(32)
```

Sample count (4-64) trades quality vs performance. Higher = smoother but slower.

## Sprite Shadows

Shadows attached directly to sprites, independent of dynamic lights.

### Drop Shadows

Artist-controlled offset shadows:

```rust
// Simple drop shadow
Renderable::sprite(handle).with_drop_shadow(DropShadow::default())

// Presets
DropShadow::below(8.0)      // Directly below
DropShadow::diagonal(6.0)   // Classic 45-degree
DropShadow::squashed(12.0, 0.3)  // Isometric style

// Custom
DropShadow::new()
    .offset(4.0, 8.0)
    .color(Color::new(0, 0, 0, 100))
    .scale(1.0, 0.6)
```

### Blob Shadows

Elliptical shadows for character grounding:

```rust
Renderable::sprite(handle).with_blob_shadow(
    BlobShadow::ellipse(20.0, 10.0)
        .offset(48.0)  // Below sprite
        .with_height_scale(true)  // Shrinks when jumping
        .max_height(150.0)
)
```

### Contact Shadows

Darkening gradient at object bases:

```rust
Renderable::sprite(handle).with_contact_shadow(
    ContactShadow::new()
        .radius(8.0)
        .intensity(0.6)
        .falloff(2.0)
)
```

## Ambient Occlusion

Screen-space post-process effect that darkens corners and crevices:

```rust
// Enable AO
ctx.effects.set_ambient_occlusion(0.3, 16.0);  // intensity, radius

// Custom with bias
ctx.effects.set_ambient_occlusion_custom(0.3, 16.0, 0.05);

// Disable
ctx.effects.disable_ambient_occlusion();
```

## Ambient Light

Global base illumination:

```rust
// Custom ambient
lighting.set_ambient(AmbientLight::new(Color::new(20, 20, 40, 255), 0.15));

// Presets
AmbientLight::dark()      // Pitch black (horror games)
AmbientLight::night()     // Dim blue
AmbientLight::moonlit()   // Blue tint
AmbientLight::indoor()    // Warm dim
AmbientLight::daylight()  // Bright
AmbientLight::sunset()    // Warm orange
```

## Volumetric Effects

### God Rays

Light beams visible in atmosphere:

```rust
Light2D::point(Color::WHITE, 200.0)
    .with_volumetric(0.3, 0.5, 0.2)  // intensity, density, scattering
    .with_god_rays()
```

### Atmospheric Scattering

Global fog/atmosphere effect:

```rust
lighting.set_atmosphere(Atmosphere {
    density: 0.3,
    scattering: 0.2,
    color: Color::new(100, 120, 150, 255),
});
```

## Light Occluders

Shapes that block light and cast shadows:

```rust
// Rectangle
LightOccluder2D::rect(width, height)

// Circle (approximated with segments)
LightOccluder2D::circle(radius, 16)

// Custom polygon (CCW winding)
LightOccluder2D::polygon(vec![
    Vec2::new(-20.0, -20.0),
    Vec2::new(20.0, -20.0),
    Vec2::new(20.0, 20.0),
    Vec2::new(-20.0, 20.0),
])
```

## Performance Tips

- **Light count** - Keep under 20-30 lights for best performance
- **Shadow casting** - Disable shadows on distant/unimportant lights
- **Occluder complexity** - Simple shapes are faster than complex polygons
- **Ray-march samples** - Use 16-32 for good quality, 64 only when needed
- **Ambient occlusion** - Lower radius = faster; disable if not needed
