# AGENTS.md - Graphics Module

## Overview

The `graphics/` module contains all rendering-related systems: sprites, animation, camera, tiled rendering, and GPU batching.

## File Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module exports and re-exports |
| `sprite.rs` | `Sprite`, `AnimatedSprite` - sprite data and CPU blitting |
| `renderer.rs` | `WgpuRenderer` - GPU rendering pipeline with letterboxing |
| `sprite_gpu.rs` | `SpriteGpuRenderer`, `BatchCollector` - GPU sprite batching |
| `camera.rs` | `Camera` - 2D camera with zoom and world/screen transforms |
| `animation.rs` | `Animation`, `Animator`, `AnimationController` |
| `tile.rs` | `TiledSprite`, `CompositeTile`, `GroupedCollider` |

## Key Types

### Sprite

Raw sprite data with CPU blitting:

```rust
// Load from file (via AssetManager)
let sprite = Sprite::load("player.png")?;

// Create programmatically
let rect = Sprite::solid_rect(32, 32, 255, 0, 0, 255);

// Draw to frame buffer
sprite.draw(frame, width, height, x, y);
sprite.draw_scaled(frame, width, height, x, y, scale);
sprite.draw_rotated(frame, width, height, x, y, angle);
```

### Camera

2D camera for world-to-screen coordinate transforms:

```rust
// Follow a target smoothly
camera.follow(target_x, target_y, 0.1);

// Zoom in/out
camera.set_zoom(2.0);

// Convert coordinates
let (screen_x, screen_y) = camera.world_to_screen(world_x, world_y);
let (world_x, world_y) = camera.screen_to_world(screen_x, screen_y);
```

### Animation System

Three-tier animation API:

```rust
// 1. Animation - frame sequence definition
let walk = Animation::new("walk", frames, 0.1)
    .with_loop_mode(LoopMode::Loop);

// 2. Animator - simple component
let animator = Animator::new(walk);

// 3. AnimationController - state machine
let mut controller = AnimationController::new();
controller.add_animation("idle", idle_anim);
controller.add_animation("walk", walk_anim);
controller.set_state("idle");
```

### Tiled Rendering

For repeating sprites and composite tiles:

```rust
// Composite tile (e.g., pipe with cap + body)
let (composite, collider) = CompositeTileBuilder::new()
    .cap(cap_sprite, 0.0, 0.0)
    .body_tiled(body_sprite, 0.0, 32.0, TileDirection::Down, 128.0)
    .build_with_collider();
```

### GPU Sprite Batching

For high-performance particle systems:

```rust
fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
    gpu.upload_sprite(&self.particle_sprite);

    for particle in &self.particles {
        let instance = SpriteInstance::new()
            .with_position(particle.x, particle.y)
            .with_rotation(particle.angle);
        gpu.batch.add(sprite_id, false, instance, 0);
    }
}
```

## Import Patterns

From within this module (sibling files):
```rust
use super::sprite::Sprite;
use super::camera::Camera;
```

From other core modules:
```rust
use crate::core::graphics::{Camera, Sprite, Animation};
use crate::core::graphics::{BatchCollector, SpriteInstance};
```

From game code via prelude:
```rust
use ryo2d::prelude::*;
// Camera, Sprite, Animation, Animator, etc. available
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     CPU Rendering Path                       │
│  Sprite.draw() → frame buffer → WgpuRenderer → screen       │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                     GPU Rendering Path                       │
│  SpriteInstance → BatchCollector → SpriteGpuRenderer → GPU  │
└─────────────────────────────────────────────────────────────┘
```

**When to use GPU path:**
- 1000+ sprites (particles, bullets)
- Per-sprite shader effects
- CPU blitting becomes bottleneck

## Notes for AI Agents

1. **CPU vs GPU rendering** - Most games use CPU path; GPU path for particles/effects
2. **Camera is separate from rendering** - Just handles coordinate transforms
3. **Animation updates automatically** - Engine updates Animator/AnimationController each frame
4. **Tiles are composable** - Use CompositeTileBuilder for complex multi-part structures
5. **Shader paths use `../shaders/`** - Graphics files reference shaders in sibling folder
