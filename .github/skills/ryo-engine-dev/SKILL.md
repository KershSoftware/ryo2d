---
name: ryo-engine-dev
description: Working on the Ryo2D engine - architecture, systems, public API.
metadata:
  version: 1.0.0
---

# Ryo2D Engine Developer

You help developers work on the Ryo2D engine itself.

## Resources

- **Project Overview**: `AGENTS.md` (root)
- **Core Modules**: `src/core/AGENTS.md`
- **ECS Design**: `src/core/ecs/AGENTS.md`
- **UI Module**: `src/core/ui/AGENTS.md`
- **CLI Design**: `src/cli/AGENTS.md`

## Architecture

```
User's Game (Game trait)
         |
         v
    GameContext
    +-------------------------------------+
    | World | Camera | Input | Assets    |
    +-------------------------------------+
         |
         v
    Engine Systems
    Animation -> Physics -> Collision -> Rendering
         |
         v
    WgpuRenderer
    CPU Pixel Buffer -> GPU Texture -> Present
```

## Module Organization

| Module | Purpose |
|--------|---------|
| `engine.rs` | Game loop, window, EngineBuilder |
| `game.rs` | Game trait, GameContext |
| `ecs/` | ECS (hecs wrapper) |
| `renderer.rs` | wgpu software renderer |
| `sprite_gpu.rs` | GPU sprite batching |
| `rapier_physics.rs` | Rapier2D integration |
| `animation.rs` | Animation system |
| `asset.rs` | AssetManager |
| `audio.rs` | Kira audio |
| `camera.rs` | 2D camera |
| `scene.rs` | Scene management |
| `ui/` | Immediate-mode UI |

## Adding Components

1. Define in `ecs/components.rs`:
```rust
#[derive(Debug, Clone)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}
```

2. Export in `ecs/mod.rs`:
```rust
pub use components::Health;
```

3. Add to prelude in `lib.rs` (if public):
```rust
pub use crate::core::ecs::Health;
```

## Adding Systems

```rust
fn update_poison_system(world: &mut World, dt: f32) {
    for (entity, (mut health, poison)) in
        world.query_mut::<(&mut Health, &Poison)>()
    {
        health.current -= poison.damage_per_second * dt;
    }
}
```

Call in engine's update loop at appropriate point.

## Public API Design

Export in `lib.rs::prelude`:
- Types users spawn/query: `Transform`, `Velocity`
- Traits users implement: `Game`, `Scene`
- Handles users store: `Entity`, `SpriteHandle`

Don't export:
- Internal engine state
- System functions
- Implementation details

## Error Handling

- No `unwrap()` in library code
- Return `Result` for fallible operations
- Provide helpful error messages
- Use centralized error types in `error.rs`

## Development Workflow

```bash
./run check    # Fast compile check
./run lint     # Clippy
./run test     # Tests
./run breakout # Test with example
```

## Tips

- Read relevant `AGENTS.md` before modifying a module
- Check example games to understand feature usage
- Keep public API minimal
- Performance matters in hot paths
- Document public types with rustdoc
