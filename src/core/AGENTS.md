# AGENTS.md - Core Engine Modules

## Overview

This folder contains all core engine functionality. Everything here is "part of the engine" - as opposed to `src/main.rs` which is just a demo.

## Module Structure

| Module | Purpose |
|--------|---------|
| `mod.rs` | Public exports for core modules |
| `engine.rs` | `Engine` struct, game loop, window management |
| `error.rs` | Unified error types (`EngineError`, `AssetError`, `SaveError`, etc.) |
| `game.rs` | `Game` trait and `GameContext` for user code |
| `scene.rs` | Scene management, transitions, scene stack |
| `workspace.rs` | Project discovery, `ryo.toml` configuration, path management |
| `debug.rs` | Debug visualization (colliders, velocities, origins, FPS) and drawing primitives |
| `ecs/` | Entity-Component-System using hecs (see `ecs/AGENTS.md`) |
| `animation.rs` | Sprite animation with `Animation`, `Animator`, `AnimationController` |
| `asset.rs` | `AssetManager` and `SpriteHandle` for resource loading/caching |
| `audio.rs` | `AudioManager`, sound effects, and music playback (Kira) |
| `camera.rs` | 2D camera with follow, zoom, coordinate transforms |
| `collision.rs` | AABB/Circle shapes, `Collider` component, collision detection |
| `hot_reload.rs` | Background filesystem watcher for dev-time asset hot reloading |
| `input.rs` | Keyboard/mouse/scroll input state tracking |
| `network/` | Client-server networking, replication, and RPCs (see `network/AGENTS.md`) |
| `physics.rs` | `RigidBody`, `Acceleration`, gravity, drag (config) |
| `rapier_physics.rs` | Rapier2D integration, physics world, collision events |
| `renderer.rs` | wgpu-based software renderer with letterbox/scaling support |
| `save.rs` | 3-tier save system with compression (system/world/game data) |
| `spatial.rs` | `SpatialHash` for broad-phase, `PhysicsWorld` integration |
| `sprite.rs` | `Sprite` and `AnimatedSprite` for 2D rendering |
| `sprite_gpu.rs` | GPU sprite batching system with per-sprite shaders and effects |
| `tile.rs` | Tiled rendering (`TiledSprite`, `CompositeTile`) and `GroupedCollider` |
| `shaders/` | WGSL shader files for GPU rendering (sprite effects, post-processing) |
| `lighting/` | 2D lighting system with point lights, spotlights, and shadows (see `lighting/AGENTS.md`) |
| `ui/` | Immediate-mode UI system for HUDs and overlays (see `ui/AGENTS.md`) |

## Import Patterns

Within `core/`, modules reference each other using `super::`:

```rust
// Inside core/engine.rs
use super::asset::AssetManager;
use super::camera::Camera;
use super::game::Game;
use super::input::InputState;
```

From outside `core/` (like from `lib.rs`), use `crate::core::`:

```rust
// Inside lib.rs
pub use crate::core::engine::EngineBuilder;
pub use crate::core::game::{Game, GameContext};
pub use crate::core::asset::{AssetManager, SpriteHandle};
```

## Design Philosophy

**Engine Owns the Loop** - User code never calls render or update directly. The `Game` trait provides callbacks that the engine invokes at the right time.

**ECS for Entity Management** - Game objects are entities with attached components. The engine provides built-in components (`Transform`, `Velocity`, `SpriteRenderer`, `RigidBody`, `Collider`) and users can add their own.

**Assets via Handles** - Assets are loaded through `AssetManager` and accessed via lightweight handles (`SpriteHandle`). Handles use `Arc` for shared ownership, enabling multiple entities to reference the same asset data.

**Minimal Public Surface** - Only types users need are exposed through `lib.rs::prelude`. Internal implementation details stay private.

## System Dependencies

```
┌─────────────────────────────────────────────────────────────┐
│                        GameContext                          │
│  ┌─────────┐ ┌────────┐ ┌────────────┐ ┌──────────────────┐ │
│  │  World  │ │ Camera │ │ InputState │ │  AssetManager    │ │
│  │  (ECS)  │ │        │ │            │ │                  │ │
│  └─────────┘ └────────┘ └────────────┘ └──────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                     Engine Systems                          │
│  Animation → Physics → Collision → Movement → Rendering     │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                      WgpuRenderer                           │
│  ┌──────────────┐ ┌────────────┐ ┌────────────────────────┐ │
│  │ Pixel Buffer │ │ GPU Texture│ │ Fullscreen Quad Shader │ │
│  │ (CPU RGBA)   │ │ (wgpu)     │ │ (WGSL)                 │ │
│  └──────────────┘ └────────────┘ └────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

## Animation System

The animation system (`animation.rs`) provides three APIs for different use cases:

### 1. Animation - Frame Sequence Definition
Defines a sequence of sprite frames with timing:
- Uniform or per-frame durations
- Loop modes: `Once`, `Loop`, `PingPong`, `Times(n)`
- Used as building blocks for animators

### 2. Animator - Simple ECS Component
For basic looping animations (coins spinning, flames flickering):
- Attach to entities with `SpriteRenderer`
- Engine automatically updates frames each tick
- Simple playback control (play/pause/stop/speed)

### 3. AnimationController - State Machine
For characters with multiple animation states:
- Manages multiple named animations
- State transitions (idle → walk → attack)
- Transition-after-finish for one-shot animations
- Current state tracking

**Integration:** The engine updates `Animator` and `AnimationController` components automatically each frame, and syncs their current sprite to the entity's `SpriteRenderer` component. Game code just sets states or swaps animations.

**Example Use Case (Flappy Bird):**
- `AnimationController` with "flap" (looping) and "hurt" (looping) states
- Game switches to "hurt" state on death
- Engine handles frame timing and sprite updates

## Tiled Rendering System

The tiled rendering system (`tile.rs`) provides components for repeating sprites and composite multi-part tiles.

### TiledSprite - Simple Repeating Tiles
Repeats a single sprite in one direction to fill a target size:
- Directions: `Up`, `Down`, `Left`, `Right`
- Automatically calculates tile count from `target_size / sprite_size`
- Useful for simple patterns like grass, walls, or ladders

### CompositeTile - Multi-Part Tiles
Combines multiple parts (static and tiled) into a single entity:
- Each `TilePart` can be static (drawn once) or tiled (repeated)
- Parts support `flip_x` and `flip_y` for sprite orientation
- Used for complex structures like pipes (cap + tiled body)

### GroupedCollider - Single Collision Box
Provides a single collision box covering an entire composite tile:
- More efficient than per-part colliders
- Auto-calculated from composite bounds via `GroupedCollider::from_composite()`
- Integrates with Rapier physics via `register_grouped_entity()`
- Supported by debug visualization (`show_colliders`)

### CompositeTileBuilder - Fluent API
Ergonomic builder for creating composite tiles with colliders:

```rust
let (composite, collider) = CompositeTileBuilder::new()
    .cap_flipped_y(cap_sprite, 0.0, 0.0)        // Flipped cap at top
    .body_tiled(body_sprite, 0.0, 24.0, TileDirection::Down, 128.0)
    .z_order(5)
    .build_with_collider();

world.spawn((
    Transform::new(x, y),
    RigidBody::kinematic(),
    composite,
    collider,
));
```

**Integration:** The engine renders `TiledSprite` and `CompositeTile` components automatically after `SpriteRenderer`, respecting z-order. Physics registration for `GroupedCollider` is automatic like standard `Collider`.

**Example Use Case (Flappy Bird Pipes):**
- Top pipe: body tiles downward, cap at bottom flipped vertically
- Bottom pipe: cap at top (normal), body tiles downward
- Single `GroupedCollider` covers entire pipe for collision

## GPU Sprite Rendering System

The GPU sprite rendering system (`sprite_gpu.rs`) provides high-performance batched rendering for scenarios where CPU software rendering is too slow.

### When to Use GPU Rendering
- **Particle systems** with 1000+ sprites
- **Bullet hell games** with hundreds of projectiles
- **Effects-heavy scenes** requiring per-sprite shaders
- **Any scenario** where CPU blitting becomes a bottleneck

### Architecture
The GPU path bypasses the CPU pixel buffer entirely:
- Sprites rendered as textured quads directly on GPU
- Automatic batching by texture and effect type
- Instance buffer for per-sprite transforms
- Support for built-in effects (outline, glow, flash, etc.)

### Usage Pattern
Games implement `render_gpu()` callback instead of relying on automatic `SpriteRenderer` rendering:

```rust
fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
    // Upload sprite textures (cached by ID)
    for sprite in &self.sprites {
        gpu.upload_sprite(sprite);
    }
    
    // Add instances to batch collector
    for entity in gpu.world.entities_with::<Particle>() {
        let transform = gpu.world.get_component::<Transform>(entity)?;
        let instance = SpriteInstance::new()
            .with_transform(x, y, rotation, scale_x, scale_y);
        gpu.batch.add(sprite.id(), has_effects, instance, z_order);
    }
}
```

### BatchCollector - Automatic Grouping
Sprites are automatically grouped by:
- **Texture ID** - Sprites with same texture batched together
- **Effect status** - Effects/no-effects use different pipelines

This minimizes GPU state changes and draw calls.

### Synchronization Fix
**Critical:** Instance data is pre-uploaded to GPU buffer **before** starting the render pass. This prevents race conditions where the GPU reads from a buffer that's still being written to.

**Implementation:** Each batch's data is written to a different offset in the instance buffer, then the render pass uses `buffer.slice(offset..end)` to point at the correct region.

### Built-in Shader Effects
Sprites can use built-in effects via `SpriteEffect`:
- **Outline** - Colored border around sprite
- **Glow** - Soft radial glow
- **Flash** - Color flash overlay
- **Dissolve** - Fade-out effect
- **Distortion** - Wave/ripple effects

Effects are specified per-instance via `SpriteInstance::with_effect()`.

**Integration:** GPU rendering happens after CPU rendering but before post-processing. The engine automatically routes sprites through GPU path when `render_gpu()` populates the batch collector.
