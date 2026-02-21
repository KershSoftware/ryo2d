# Per-Sprite Shaders & Materials

Ryo2D provides a flexible shader system for applying visual effects to individual sprites. This is distinct from [post-processing effects](post-processing.md) which operate on the entire frame.

## Quick Start

```rust
use ryo2d::prelude::*;

// Inline shader on Renderable
let highlighted = Renderable::sprite(item_handle)
    .with_shader(SpriteShader::outline(Color::WHITE, 2));

// Or as a separate Material component
world.spawn((
    Transform::new(100.0, 200.0),
    Renderable::sprite(enemy_handle),
    Material::damage_flash(0.8),
));
```

## Two Ways to Attach Shaders

### 1. Inline on Renderable

Use `.with_shader()` for simple, one-off effects:

```rust
let player = Renderable::sprite(sprite)
    .with_shader(SpriteShader::glow(Color::CYAN, 8.0));
```

**Pros:** Concise, everything in one component  
**Cons:** Harder to query, swap, or share between entities

### 2. Material Component

Add a separate `Material` component for more control:

```rust
world.spawn((
    Transform::new(100.0, 200.0),
    Renderable::sprite(sprite),
    Material::outline(Color::WHITE, 2),
));
```

**Pros:** Queryable, hot-swappable, reusable across entities  
**Cons:** More verbose

> **Priority:** If both inline shader and Material component are present, the **Material takes precedence**.

## Built-in Effects

Ryo2D includes 11 ready-to-use effects:

| Effect | Description | Example |
|--------|-------------|---------|
| **Outline** | Colored border around sprite | `SpriteShader::outline(Color::WHITE, 2)` |
| **Glow** | Soft radial glow effect | `SpriteShader::glow(Color::CYAN, 8.0)` |
| **Flash** | Color overlay (damage, power-up) | `SpriteShader::damage_flash(0.8)` |
| **ColorShift** | Tint toward a color | `SpriteShader::color_shift(Color::RED, 0.5)` |
| **Silhouette** | Solid color preserving alpha | `SpriteShader::silhouette(Color::BLACK)` |
| **Pixelate** | Blocky pixelation | `SpriteShader::pixelate(4)` |
| **Wave** | Animated wave distortion | `SpriteShader::wave(2.0, 5.0, 3.0)` |
| **Dissolve** | Noise-based fade out | `SpriteShader::dissolve(0.5)` |
| **Invert** | Invert colors | `SpriteShader::invert()` |
| **Grayscale** | Desaturate to grayscale | `SpriteShader::grayscale(1.0)` |

### Convenience Constructors

Common patterns have shorthand constructors:

```rust
// Damage/hit feedback
Material::damage_flash(0.8)  // Red flash
Material::hit_flash(0.6)     // White flash

// Selection/highlighting
Material::white_outline(2)   // White border

// Shadows
Material::shadow()           // Black semi-transparent silhouette

// Dissolve with edge glow
Material::dissolve_with_edge(0.5, Color::ORANGE)
```

### Animated Effects

Some effects are time-based and need updating:

```rust
// Wave effect animates over time
if let Some(material) = world.get_component_mut::<Material>(entity) {
    material.update(dt);
}
```

The `update(dt)` method returns `true` if the shader state changed.

## Custom Shaders

For unique effects, load your own WGSL shaders:

```rust
// Load custom shader
let dissolve = ShaderHandle::from_source("dissolve", include_str!("dissolve.wgsl"));

// Create material with parameters
let material = Material::custom(dissolve)
    .with_param("progress", 0.5)
    .with_param("edge_width", 0.1)
    .with_param("noise_scale", 4.0);

world.spawn((
    Transform::new(100.0, 200.0),
    Renderable::sprite(sprite),
    material,
));
```

### Shader Parameters

Parameters are passed as named `f32` values via `ShaderParams`:

```rust
let mut params = ShaderParams::new();
params.set("progress", 0.5);
params.set("intensity", 1.0);

// Query parameters
let progress = params.get("progress");        // Some(0.5)
let missing = params.get_or("missing", 0.0);  // 0.0 (default)
```

Parameters are converted to a 16-float uniform array (alphabetically sorted by name).

## Querying Entities by Material

Materials are regular ECS components, so you can query them:

```rust
// Find all entities with a Material
for (entity, material) in world.query::<&Material>() {
    if material.is_builtin() {
        // Handle built-in effect
    } else if material.is_custom() {
        // Handle custom shader
    }
}

// Update all animated materials
for (_, material) in world.query_mut::<&mut Material>() {
    material.update(dt);
}
```

## Hot-Swapping Effects

Since Material is a component, you can swap effects at runtime:

```rust
// Remove current material
world.remove_component::<Material>(entity);

// Add new material
world.add_component(entity, Material::glow(Color::GOLD, 12.0));
```

## Performance Notes

- **GPU Rendering Required:** Per-sprite shaders use GPU-based rendering. When a sprite has a shader, it will be rendered via the GPU pipeline.
- **Batching:** Sprites with the same shader can be batched together for efficiency (future: 7.6b GPU Instance Batching).
- **Built-in vs Custom:** Built-in effects are optimized and share common shader code. Custom shaders may require separate draw calls.

## When to Use What

| Scenario | Recommendation |
|----------|----------------|
| Simple highlight on pickup | Inline `with_shader()` |
| Enemy damage state (queryable) | Material component |
| Many similar particles | Share one Material reference |
| Unique boss effect | Custom shader via `Material::custom()` |
| Frame-wide effect (fade, vignette) | Post-processing, not per-sprite |

## Example: Damage Flash System

```rust
fn apply_damage(world: &mut World, entity: Entity, damage: f32) {
    // Apply damage to health...
    
    // Add visual feedback
    if world.has_component::<Material>(entity) {
        world.remove_component::<Material>(entity);
    }
    world.add_component(entity, Material::damage_flash(0.8));
}

fn update_damage_effects(world: &mut World, dt: f32) {
    for (entity, material) in world.query_mut::<&mut Material>() {
        // Fade out damage flash
        if let Some(effect) = material.shader.as_builtin_mut() {
            if let SpriteEffect::Flash { intensity, .. } = effect {
                *intensity = (*intensity - dt * 2.0).max(0.0);
                if *intensity <= 0.0 {
                    // Remove when faded
                    // (Note: can't modify world during iteration)
                }
            }
        }
    }
}
```

## See Also

- [Post-Processing Effects](post-processing.md) - Frame-wide visual effects
- [Rendering Overview](rendering.md) - How the render pipeline works
- [Sprite System](sprites.md) - Loading and displaying sprites
