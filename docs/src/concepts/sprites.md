# Sprites & Rendering

ryo2d uses a simple sprite-based rendering system with the `Renderable` component.

## Creating Sprites

### Solid Color Rectangle

```rust
// Create a 32x32 cyan rectangle (RGBA)
let sprite = Sprite::solid_rect(32, 32, 0x40, 0xc0, 0xff, 0xff);
```

### From Image File

```rust
let sprite = Sprite::load("assets/player.png")?;
```

Supported formats: PNG, JPEG, and other formats supported by the `image` crate.

## Renderable Component

Attach a sprite to an entity for rendering:

```rust
let player = ctx.world.spawn((
    Transform::new(100.0, 100.0),
    Renderable::sprite(SpriteHandle::from_sprite(sprite, "player")),
));
```

### Z-Ordering

Control draw order with z_order (lower = behind, higher = in front):

```rust
// Background (drawn first)
Renderable::sprite(bg_handle).with_z_order(-100)

// Player (drawn on top)
Renderable::sprite(player_handle).with_z_order(10)
```

### Visibility

Toggle sprite visibility:

```rust
if let Some(mut renderable) = ctx.world.get_component_mut::<Renderable>(entity) {
    renderable.visible = false;  // Hide
}
```

## Render Layers

Use preset layers for common cases:

```rust
use ryo2d::prelude::RenderLayer;

Renderable::sprite(handle).with_z_order(RenderLayer::Background.as_z_order())
Renderable::sprite(handle).with_z_order(RenderLayer::Default.as_z_order())
Renderable::sprite(handle).with_z_order(RenderLayer::Foreground.as_z_order())
Renderable::sprite(handle).with_z_order(RenderLayer::UI.as_z_order())
```

## Shader Effects

Add per-sprite visual effects:

```rust
// Outline effect
Renderable::sprite(handle)
    .with_shader(SpriteShader::outline(Color::WHITE, 2))

// Damage flash
Renderable::sprite(handle)
    .with_shader(SpriteShader::damage_flash())
```

See [Sprite Shaders](sprite-shaders.md) for all available effects.

## Alpha Blending

Sprites with alpha channels are automatically blended. Use RGBA values where the last component is alpha (0 = transparent, 255 = opaque).
