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
User's Scene (Scene trait)
         |
         v
    SceneContext
    +----------------------------------------------------+
    | Resources | World | Camera | Input | Assets | ...  |
    +----------------------------------------------------+
         |
         v
    Engine Systems (engine.rs update)
    Animation -> Tweens -> Skeleton -> Knockback -> SquashStretch
         -> Scene update -> Hierarchy -> Trails -> Physics (Rapier)
         |
         v
    Frame render (render/, one pass per stage)
    Clear -> World (entities, sprites, trails/lines, one z-sorted list; equal z: entities, sprites, trails/lines) -> Lighting
         -> Unlit lines -> Transition -> Post-process effects -> UI -> Composite -> Present
```

The frame is drawn by the pass-based `RenderPipeline` (`render/`); see `src/core/render/AGENTS.md`.

## Module Organization

| Module | Purpose |
|--------|---------|
| `engine.rs` | Game loop, window, EngineBuilder |
| `game.rs` | GpuRenderContext, TimeControl, TimeInfo, WindowRequest |
| `ecs/` | ECS (hecs wrapper), pooling, tweens |
| `render/` | Pass-based render pipeline the engine draws every frame with |
| `render/pipeline.rs` | RenderPipeline, pass execution |
| `render/pass.rs` | RenderPass trait |
| `render/passes/` | Built-in pass implementations (upload, clear, raycast, sprites, lighting, post-process, composite, ...) |
| `graphics/` | wgpu renderer, GPU sprite batching, camera, sprite animation |
| `physics/` | Rapier2D integration, colliders |
| `systems/` | Per-frame engine systems (animation, tween, physics, ...) |
| `asset/` | AssetManager, hot reload |
| `audio.rs` | Kira audio |
| `graphics/camera.rs` | 2D camera with shake |
| `scene.rs` | Scene management |
| `ui/` | Hybrid immediate/retained UI |
| `lighting/` | 2D lighting system |
| `trail/` | Motion trail rendering |

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
    for (health, poison) in world.query_mut::<(&mut Health, &Poison)>() {
        health.current -= poison.damage_per_second * dt;
    }
}
```

Call in engine's update loop at appropriate point.

## Public API Design

Export in `lib.rs::prelude`:
- Types users spawn/query: `Transform`, `Velocity`
- Traits users implement: `Scene`
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
./dev check    # Fast compile check
./dev lint     # Clippy
./dev test     # Tests
./dev breakout # Test with example
```

## Profiler Integration

The engine includes a built-in profiler using puffin:

### Key Files
- `profiler.rs` - Macros (`profile_function!`, `profile_scope!`) and init
- `windows/profiler_window.rs` - Standalone flamegraph window (CPU-rendered)

### Adding Profiler Scopes to Engine Systems

```rust
use crate::profile_scope;

fn my_engine_system(world: &mut World, dt: f32) {
    profile_scope!("my_system");
    // ... system work
}
```

### Profiler Window Architecture

ProfilerWindow is a separate wgpu window that:
1. Registers as a puffin sink to receive frame data
2. Collects scope entries from `FrameView`
3. CPU-renders colored bars to a pixel buffer
4. Uploads to GPU texture and presents

Key design decisions:
- **10 FPS throttle** - Keeps display readable and stable
- **Scope persistence** - Bars fade slowly instead of flickering
- **Separate focus tracking** - Game doesn't throttle when profiler is focused
- **Wall-clock FPS** - Shows actual frame rate, not just work time

### Testing with Profiler

```bash
./dev breakout -p
# Or:
ryo2d dev --profiler
```

## Tips

- Read relevant `AGENTS.md` before modifying a module
- Check example games to understand feature usage
- Keep public API minimal
- Performance matters in hot paths
- Document public types with rustdoc

## Verifying Rendering Changes

Use frame capture (`src/core/capture/`) instead of guessing: run an example with `RYO_CAPTURE_FRAMES=<n> RYO_CAPTURE_EXIT=1 RYO_CAPTURE_DIR=<abs dir>` and inspect the PNG. Capture before and after a renderer change to prove it. Add `RYO_HEADLESS=1` to run without opening a window, and `RYO_INPUT="60:Digit2 120:ArrowRight"` to reach pages behind keys (see the `ryo-cli` skill).

## Demos

Feature demos follow one standard (shared keys, settings panel, layout) through `examples/demo-kit`; see `examples/demos/AGENTS.md`. `./dev demos:tour` captures every page of every demo headlessly to `target/demo-tour/`.

## Headless Engine Tests

Regression tests for rendering go in `tests/e2e/` using `ryo2d::testing::HeadlessEngine` (see `src/core/testing/AGENTS.md`). Write the failing pixel assertion first, then fix.
