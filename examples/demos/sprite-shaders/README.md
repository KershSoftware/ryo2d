# Sprite Shaders Demo

This demo showcases all per-sprite shader effects available in Ryo2D.

## Effects Demonstrated

1. **Outline** - Draw colored borders around sprites
2. **Glow** - Add bloom/halo effects
3. **Flash** - Damage/power-up flashes
4. **ColorShift** - Status effect tints
5. **Silhouette** - Shadow/death effects
6. **Pixelate** - Retro/transition effects
7. **Wave** - Water/heat shimmer
8. **Dissolve** - Death/teleport animations
9. **Invert** - Color inversion
10. **Grayscale** - Desaturation

## Controls

| Key | Action |
|-----|--------|
| ← / → | Navigate between effects |
| ↑ / ↓ | Adjust primary parameter |
| C | Cycle effect color |
| Space | Trigger animated effects (Flash, Dissolve) |
| R | Reset parameters to defaults |
| Escape | Quit |

## Usage Patterns

### Inline on Renderable

```rust
let player = Renderable::sprite(sprite)
    .with_shader(SpriteShader::outline(Color::WHITE, 2));
```

### Material Component (reusable, queryable)

```rust
world.spawn((
    Transform::new(100.0, 200.0),
    Renderable::sprite(sprite),
    Material::outline(Color::WHITE, 2),
));
```

### Convenience Constructors

```rust
SpriteEffect::damage_flash(0.7)   // Red flash at 70%
SpriteEffect::hit_flash(0.8)     // White flash at 80%
SpriteEffect::white_outline(2)   // White 2px outline
SpriteEffect::shadow()           // Semi-transparent black silhouette
SpriteEffect::full_grayscale()   // Complete desaturation
```

## Running

```bash
cd examples/demos/sprite-shaders
cargo run
```
