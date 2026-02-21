# Profiling & Performance

This guide covers how to profile your Ryo2D games to identify performance bottlenecks and optimize your game.

## Runtime Debug Key Toggles

During development, you can toggle debug overlays without restarting:

| Key | Action |
|-----|--------|
| **F1** | Cycle overlay mode: Off → FPS → Frame Timing → Full Stats → Off |
| **F2** | Toggle collision boundaries |
| **F3** | Toggle velocity vectors |
| **F4** | Toggle origin points |

To disable these keys (e.g., for a release build), set `disable_debug_keys = true` in your `Ryo.toml`.

## Built-in Frame Timing

Ryo2D includes a built-in frame timing overlay that shows exactly where your game spends time each frame. Press **F1** to cycle through overlay modes, or enable it in your `Ryo.toml`:

```toml
[debug]
show_frame_timing = true
```

This displays a breakdown in the corner of your screen:

```
Update:  2.3ms
Render:  1.8ms
Present: 0.4ms
─────────────
Total:   4.5ms (131 FPS)
```

### What Each Measurement Means

| Phase | Description |
|-------|-------------|
| **Update** | Your game logic + physics simulation combined |
| **Render** | CPU-side sprite rendering and GPU batch preparation |
| **Present** | GPU submission, effects pipeline, and vsync wait |

### Quick Diagnosis

| Symptom | Likely Cause | Solution |
|---------|--------------|----------|
| High Update | Complex game logic or physics | Optimize algorithms, reduce physics bodies |
| High Render | Too many draw calls | Use GPU sprite batching, reduce overdraw |
| High Present | GPU bottleneck or vsync | Reduce resolution, simplify shaders |

## Debug Statistics Overlay

For deeper insight, enable the full statistics overlay by pressing **F1** until "Full Stats" mode, or configure in `Ryo.toml`:

```toml
[debug]
show_frame_timing = true
show_stats = true
```

This adds entity and rendering statistics:

```
Entities:   142
Sprites:    238
Batches:      3
─────────────
Update:  2.3ms
Render:  1.8ms
Present: 0.4ms
─────────────
Total:   4.5ms (131 FPS)
```

### Statistics Explained

| Stat | Description | Optimization Target |
|------|-------------|---------------------|
| **Entities** | Active ECS entities | Fewer = faster iteration |
| **Sprites** | Sprites submitted for rendering | Cull off-screen sprites |
| **Batches** | GPU draw calls | Lower = better (batch by texture) |

## FPS Counter

For a simpler view, enable just the FPS counter:

```toml
[info]
show_fps = true
```

## Frame Timing

Ryo2D always captures detailed frame timing measurements with negligible overhead (~200-400 nanoseconds per frame). Use **F1** to cycle through overlay modes, or configure in `Ryo.toml`.

### Debug Build Warnings

In debug builds, Ryo2D automatically prints warnings to help identify performance issues:

- **Slow frame warnings** - Logged when a frame takes >33ms (below 30 FPS)
- **Scene load warnings** - Logged when `on_enter()` takes >5ms

## In-Game Profiler Window (F10)

Ryo2D includes a built-in profiler window that displays a real-time flamegraph directly in your game. Press **F10** to toggle it (requires the `profiler` feature).

### Enabling the Profiler

**Option 1: CLI with auto-open (quick profiling)**

```bash
ryo2d dev --profiler
# or shorthand:
ryo2d dev -p
```

This compiles with the `profiler` feature and **auto-opens** the profiler window on launch.

**Option 2: Ryo.toml configuration (regular development)**

```toml
[debug]
enable_profiler = true
```

This compiles with the `profiler` feature but doesn't auto-open. Press **F10** to open the profiler window when needed. This is ideal when you only occasionally need the profiler.

### Using the Profiler Window

Once running with the profiler feature, press **F10** to open the profiler window (or it opens automatically with `-p` flag). The window shows:

- **Real-time flamegraph** - Hierarchical view of where time is spent each frame
- **Scope names** - Function/scope labels on each bar
- **Frame duration** - Total frame time displayed in the header
- **Pause/Resume** - Click the pause button to freeze the display for analysis

**Controls:**
- **Pause/Resume button** - Freeze the flamegraph to analyze a specific frame
- **Hover** - See exact timing for each scope
- The window can be opened/closed at any time without affecting your game

### What You'll See

The flamegraph displays nested scopes from the engine and your game code:

```
┌─────────────────────────────── ENGINE ────────────────────────────────┐
│ ┌─────────── update ───────────┐ ┌─────── render ───────┐ ┌─present─┐ │
│ │ ┌─physics─┐ ┌─game_logic─┐   │ │ ┌─sprites─┐ ┌─ui─┐   │ │         │ │
│ │ │         │ │            │   │ │ │         │ │    │   │ │         │ │
```

- **Wider bars** = more time spent
- **Nested bars** = child scopes called within parent
- **ENGINE** = top-level frame scope

### Tips

- Open the profiler when you notice frame drops to identify the cause
- The profiler window itself has minimal overhead
- Works alongside the F1 frame timing overlay for quick comparisons
- Scope names update correctly even if you open the profiler mid-session

## External Puffin Viewer

For an external profiler window (useful for multi-monitor setups), you can use `puffin_viewer`:

**First-time setup:**

```bash
cargo install puffin_viewer
```

**Launch manually:**

```bash
puffin_viewer --url 127.0.0.1:8585
```

When using `ryo2d dev --profiler`, the CLI automatically launches `puffin_viewer` if installed.

The external viewer provides the same flamegraph visualization with additional features:
- **Frame history** with timeline navigation
- **Pause/resume** for inspecting specific frames
- Runs in a separate window

### Instrumenting Your Own Code

Add profiling to your game code using the macros from the prelude:

```rust,ignore
use ryo2d::prelude::*;

fn my_expensive_function() {
    profile_function!();  // Profiles entire function

    // ... some work ...

    {
        profile_scope!("particle_update");  // Profiles just this scope
        for particle in &mut particles {
            particle.update(dt);
        }
    }
}
```

When the `profiler` feature is disabled (default), these macros compile to nothing with zero overhead.

### Pre-Instrumented Engine Scopes

The engine automatically instruments these scopes:
- `update` - Game logic and all fixed timestep work
- `render` - CPU sprite rendering and batch collection
- `present` - GPU submission and effects pipeline
- `physics_step` - Rapier2D physics simulation
- `scene_push` / `scene_switch` - Scene transitions

### Interpreting Puffin Data

| Scope | High Time Indicates |
|-------|---------------------|
| `update` | Heavy game logic, too many entities |
| `render` | Complex sprite rendering, large textures |
| `present` | GPU-bound, effects overhead, vsync |
| `physics_step` | Too many colliders, complex shapes |
| `scene_*` | Expensive asset loading in on_enter |

## Advanced: External Flamegraph Profiling

> **Note:** This section covers external profiling tools for deep performance analysis. These require additional setup and administrator privileges on some platforms. Most performance issues can be diagnosed with the built-in frame timing overlay.

For detailed CPU profiling showing the full call stack, you can use `cargo-flamegraph`:

### Installation

Install `cargo-flamegraph`:

```bash
cargo install flamegraph
```

**Platform-specific requirements:**

- **Windows:** Install [Windows Performance Toolkit](https://docs.microsoft.com/en-us/windows-hardware/test/wpt/) (part of Windows SDK)
- **Linux:** Install `perf` (`sudo apt install linux-tools-generic` on Ubuntu)
- **macOS:** Uses DTrace (built-in, may require SIP adjustment)

### Generating a Flamegraph

Ryo2D includes debug symbols in release builds by default, so you can profile optimized code with readable function names.

```bash
# Profile your game
cd examples/games/breakout
cargo flamegraph --release

# Or use the profiling profile for maximum symbol info
cargo flamegraph --profile profiling
```

This runs your game and generates `flamegraph.svg` when you close it.

### Reading Flamegraphs

![Flamegraph Example](https://www.brendangregg.com/FlameGraphs/cpu-bash-flamegraph.svg)

- **Width** = time spent (wider = more time)
- **Height** = call stack depth (bottom = entry point, top = leaf functions)
- **Color** = random (no meaning, just for visual distinction)

**Tips:**
- Look for wide boxes at the top - these are hot functions
- Click to zoom into a subtree
- Search (Ctrl+F) for specific function names

### Common Bottlenecks

| Symptom | Likely Cause | Solution |
|---------|--------------|----------|
| Wide `wgpu::*` blocks | GPU sync/submission | Reduce draw calls, batch sprites |
| Wide `rapier2d::*` blocks | Physics simulation | Fewer bodies, simpler colliders |
| Wide `hecs::*` blocks | ECS queries | Cache query results, use sparse sets |
| Wide `image::*` blocks | Texture loading | Load assets during loading screen |

## Profile Configurations

Ryo2D's `Cargo.toml` includes two profiling-friendly configurations:

### Release with Debug Symbols (Default)

```toml
[profile.release]
debug = true
```

Standard release optimizations with function names preserved. Good for most profiling.

### Profiling Profile

```toml
[profile.profiling]
inherits = "release"
debug = true
```

Use with `--profile profiling` for dedicated profiling sessions.

## Profiling Workflow

1. **Start with built-in timing** - Enable `show_frame_timing` to identify which phase is slow
2. **If you need deeper analysis** - Use flamegraph for call-stack profiling
3. **Analyze** - Look for unexpectedly wide functions in the flamegraph
4. **Fix** - Make a targeted change
5. **Verify** - Check frame timing to confirm improvement

## Platform Notes (Flamegraph)

### Windows

If you see "Access Denied" errors, run your terminal as Administrator.

### Linux

You may need to adjust `perf_event_paranoid`:

```bash
echo 1 | sudo tee /proc/sys/kernel/perf_event_paranoid
```

### macOS

DTrace requires partial SIP (System Integrity Protection) disable for full functionality. For basic profiling, you can often use Instruments instead.

## Optimization Patterns

### Entity Management

```rust,ignore
// Bad: Creating/destroying entities every frame
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    for _ in 0..100 {
        ctx.world.spawn(...);  // Allocation overhead
    }
    SceneAction::Continue
}

// Good: Use object pooling for frequently spawned entities
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    let bullet = self.bullet_pool.acquire(ctx.world);
    // Reuse existing entity, just update components
    SceneAction::Continue
}
```

### Draw Call Batching

```rust,ignore
// Bad: Many small textures = many draw calls
// sprites_a.png, sprites_b.png, sprites_c.png...

// Good: Pack sprites into atlas = fewer batches
// Use a sprite sheet with all game sprites
// Ryo2D auto-batches consecutive sprites from same texture
```

### Physics Optimization

```rust,ignore
// Bad: Complex polygon colliders
Collider::polygon(100_vertices);

// Good: Simple primitives
Collider::circle(radius);  // Fastest
Collider::aabb(width, height);  // Fast
```

### Query Caching

```rust,ignore
// Bad: Re-querying every frame
fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    for entity in ctx.world.entities_with::<Enemy>() {
        // Query runs every frame
    }
    SceneAction::Continue
}

// Good: Cache entity lists when possible
struct GameState {
    enemies: Vec<Entity>,  // Updated only when enemies spawn/despawn
}
```

## Common Performance Issues

| Issue | Diagnosis | Solution |
|-------|-----------|----------|
| Frame drops on spawn | High Update spike | Use object pooling |
| Stuttering every second | GC or asset loading | Preload assets, avoid allocations |
| Gradual slowdown | Memory leak or entity buildup | Check entity count, despawn unused |
| GPU bottleneck | High Present, low Update/Render | Reduce resolution, simplify shaders |
| Physics hitching | Many bodies created at once | Stagger spawns, use triggers for non-physics |

## Profiling Checklist

Before shipping:

- [ ] Run with `show_frame_timing = true` and check all phases are within budget
- [ ] Test with maximum expected entity count
- [ ] Profile with `cargo flamegraph --release` for CPU bottlenecks
- [ ] Check entity count doesn't grow unbounded over time
- [ ] Verify no slow frame warnings during normal gameplay
- [ ] Test on minimum spec hardware

## See Also

- [Debugging](./debugging.md) - Runtime debug overlays
- [Game Loop](./game-loop.md) - Understanding the update/render cycle
- [ECS Architecture](./ecs.md) - Entity management best practices
