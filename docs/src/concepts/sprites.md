# Sprites & Rendering

Ferrum2D uses a simple sprite-based rendering system.

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

## SpriteRenderer Component

Attach a sprite to an entity:

```rust
let player = ctx.world.spawn();
ctx.world.add_component(player, Transform::new(100.0, 100.0));
ctx.world.add_component(player, SpriteRenderer::new(sprite));
```

### Z-Ordering

Control draw order with z_order (lower = behind, higher = in front):

```rust
// Background (drawn first)
SpriteRenderer::new(bg_sprite).with_z_order(-100)

// Player (drawn on top)
SpriteRenderer::new(player_sprite).with_z_order(10)
```

### Visibility

Toggle sprite visibility:

```rust
if let Some(mut renderer) = ctx.world.get_component_mut::<SpriteRenderer>(entity) {
    renderer.visible = false;  // Hide
}
```

## Render Layers

Use preset layers for common cases:

```rust
use ferrum2d::prelude::RenderLayer;

SpriteRenderer::new(sprite).with_z_order(RenderLayer::Background.as_z_order())
SpriteRenderer::new(sprite).with_z_order(RenderLayer::Default.as_z_order())
SpriteRenderer::new(sprite).with_z_order(RenderLayer::Foreground.as_z_order())
SpriteRenderer::new(sprite).with_z_order(RenderLayer::UI.as_z_order())
```

## Alpha Blending

Sprites with alpha channels are automatically blended. Use RGBA values where the last component is alpha (0 = transparent, 255 = opaque).
