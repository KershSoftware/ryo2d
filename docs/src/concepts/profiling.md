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

## Profiling Configuration

By default, Ryo2D captures detailed timing measurements in development builds. You can control this with the `enable_profiling` option:

```toml
[debug]
enable_profiling = true   # Default: true in dev builds
```

### What Gets Gated Behind `enable_profiling`

When **enabled** (default):
- Per-phase frame timing (update/render/present breakdown)
- Scene `on_enter()` timing warnings (>5ms prints to console)
- Slow frame warnings to console (>33ms frames)

When **disabled**:
- Timing fields stay at zero (overlay shows zeros for phases)
- No console warnings for slow frames or scene loads
- Basic FPS counting still works (lightweight)

### Performance Impact

The profiling overhead is negligible (~200-400 nanoseconds per frame):

| Operation | Cost | Frequency |
|-----------|------|-----------|
| `Instant::now()` | ~20-50ns | 4-6 per frame |
| `elapsed()` | ~20-50ns | 4-6 per frame |
| Duration assignment | ~1ns | 4-6 per frame |

At 60 FPS, this adds ~24 microseconds/second (**0.002% of frame time**). Safe to leave enabled during development.

### When to Disable Profiling

- **Release builds**: Disable to prevent any console output
- **Benchmarking**: Disable for cleaner measurements
- **Console-sensitive environments**: Disable to reduce log noise

## Advanced: Puffin Hierarchical Profiling

For detailed hierarchical profiling with a real-time flamegraph viewer, Ryo2D supports [puffin](https://github.com/EmbarkStudios/puffin). This provides more detailed call-stack information than the built-in frame timing overlay.

### Enabling Puffin

**Recommended: Use the ryo2d CLI**

```bash
ryo2d dev --profiler
# or shorthand:
ryo2d dev -p
```

This automatically compiles with the `profiler` feature and enables the puffin server.

**Alternative: Ryo.toml configuration**

For persistent profiler configuration, add to your `Ryo.toml`:

```toml
[debug]
puffin_profiler = true
```

Then run with:

```bash
ryo2d dev
# or: cargo run --features profiler
```

When enabled, the engine:
1. Starts a puffin HTTP server on port 8585
2. Instruments key engine systems (update, render, present, physics, scene transitions)
3. Prints connection instructions to the console

### Built-in Profiler Window

When using `ryo2d dev --profiler`, a dedicated profiler window opens automatically alongside your game. No external tools required!

The profiler window shows:
- **Real-time flamegraph** - Color-coded bars showing where time is spent
- **FPS and work time** - Actual frame rate and CPU work per frame
- **Hover tooltips** - Mouse over any bar to see exact timing
- **Setup hints** - If no profiling data appears, shows instrumentation examples

#### Profiler Window Features

| Feature | Description |
|---------|-------------|
| **Color-coded scopes** | Each scope gets a consistent color based on its name |
| **Hierarchical depth** | Nested scopes appear on lower rows |
| **Smooth display** | Bars fade gradually instead of flickering |
| **Separate focus** | Game runs normally even when profiler window is focused |
| **10 FPS refresh** | Throttled updates for stable, readable display |

#### Reading the Profiler

The header shows:
- **FPS** - Actual frames per second (including vsync wait)
- **Work time** - CPU time spent on game logic (excludes vsync)

The flamegraph shows:
- **Bar width** = Time spent (wider = more time)
- **Bar depth** = Call nesting (top = outermost scope)
- **Bar color** = Scope identity (same scope = same color)

#### Alternative: External puffin_viewer

If you prefer the external puffin viewer with frame history navigation:

```bash
# First-time setup
cargo install puffin_viewer

# Manual launch (profiler also starts HTTP server on port 8585)
puffin_viewer --url 127.0.0.1:8585
```

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

### Troubleshooting the Profiler

#### "No profiling data detected"

The profiler window shows setup instructions when no scopes are found:

1. **Check your code has profiling macros:**
   ```rust
   fn update(&mut self, ctx: &mut GameContext, dt: f32) {
       profile_function!();  // Add this!
       // ... your code
   }
   ```

2. **Verify profiler is enabled:**
   - Use `ryo2d dev --profiler` (not just `cargo run`)
   - Or set `puffin_profiler = true` in Ryo.toml AND use `--features profiler`

3. **Wait a moment** - The profiler shows "Waiting for profiling data..." for the first few seconds

#### Profiler window doesn't open

1. Check console for `[Profiler] Window opened` message
2. Window may be minimized or behind the game window
3. Ensure you're using `ryo2d dev --profiler` (the `--profiler` flag is required)

#### Bars are flickering or unstable

This is normal for very fast scopes (sub-millisecond). The profiler smooths values but inherently fast operations will vary. Focus on the wider bars that represent significant work.

#### FPS seems wrong

The profiler shows **actual wall-clock FPS**, which includes vsync wait time. If your game targets 60 FPS with vsync, you'll see ~60 FPS even if work only takes 2ms. The "Work" time shows actual CPU time.

#### Profiler impacts game performance

The built-in profiler is designed for minimal impact:
- ~200-400ns overhead per `profile_scope!()` call
- Window renders at 10 FPS to minimize GPU contention
- When profiler feature is disabled, zero overhead

If you notice impact, reduce the number of fine-grained scopes or profile only specific systems.

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
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    for _ in 0..100 {
        ctx.world.spawn(...);  // Allocation overhead
    }
}

// Good: Use object pooling for frequently spawned entities
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    let bullet = self.bullet_pool.acquire(ctx.world);
    // Reuse existing entity, just update components
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
fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    for entity in ctx.world.entities_with::<Enemy>() {
        // Query runs every frame
    }
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
