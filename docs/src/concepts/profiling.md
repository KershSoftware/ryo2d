# Profiling & Performance

This guide covers how to profile your Ryo2D games to identify performance bottlenecks and optimize your game.

## Built-in Frame Timing

Ryo2D includes a built-in frame timing overlay that shows exactly where your game spends time each frame. Enable it in your `Ryo.toml`:

```toml
[debug]
show_frame_timing = true
```

This displays a breakdown in the corner of your screen:

```
Update:  2.3ms
Physics: 3.1ms
Render:  1.8ms
Present: 0.4ms
─────────────
Total:   7.6ms (131 FPS)
```

### What Each Measurement Means

| Phase | Description |
|-------|-------------|
| **Update** | Your game logic (`Game::update`) |
| **Physics** | Rapier2D physics step |
| **Render** | Drawing sprites, UI, and effects |
| **Present** | GPU submission and vsync wait |

### Quick Diagnosis

| Symptom | Likely Cause | Solution |
|---------|--------------|----------|
| High Update | Complex game logic | Optimize algorithms, cache results |
| High Physics | Too many physics bodies | Fewer bodies, simpler colliders, spatial partitioning |
| High Render | Too many draw calls | Use GPU sprite batching, reduce overdraw |
| High Present | GPU bottleneck | Reduce resolution, simplify shaders |

## FPS Counter

For a simpler view, enable just the FPS counter:

```toml
[info]
show_fps = true
```

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

## See Also

- [Debugging](./debugging.md) - Runtime debug overlays
- [Game Loop](./game-loop.md) - Understanding the update/render cycle
