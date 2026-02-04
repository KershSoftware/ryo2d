---
name: ryo-profiler
description: Profiling Ryo2D games and engine - puffin integration, profiler window, performance analysis.
metadata:
  version: 1.0.0
---

# Ryo2D Profiler Specialist

You help developers profile and optimize Ryo2D games and the engine itself.

## Quick Start

```bash
# Run game with profiler window
ryo2d dev --profiler
# or shorthand:
ryo2d dev -p
```

A second window opens showing real-time flamegraphs.

## Profiler Window Features

The built-in profiler window (`profiler_window.rs`) provides:

- **Real-time flamegraph** - Color-coded scope bars showing time spent
- **FPS and frame time display** - Shows actual FPS and work time per frame
- **Hover tooltips** - Mouse over any bar to see scope name and duration
- **Auto-instrumentation hints** - Shows setup instructions if no scopes detected
- **Scope persistence** - Bars fade smoothly rather than flickering

## Instrumenting Code

```rust
use ryo2d::prelude::*;

fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    profile_function!();  // Profile entire function

    {
        profile_scope!("physics");
        // physics work...
    }

    {
        profile_scope!("ai_update");
        // AI work...
    }
}
```

When `profiler` feature is disabled (default), macros compile to nothing.

## Pre-Instrumented Engine Scopes

These engine systems are automatically profiled:

| Scope | What it measures |
|-------|-----------------|
| `update` | Game logic + fixed timestep |
| `render` | CPU sprite rendering |
| `present` | GPU submission + effects |
| `physics_step` | Rapier2D simulation |
| `scene_push` | Scene push transitions |
| `scene_switch` | Scene switch transitions |

## Configuration

### Via CLI (recommended)
```bash
ryo2d dev --profiler
```

### Via Ryo.toml
```toml
[debug]
puffin_profiler = true
```

Then run with profiler feature:
```bash
cargo run --features profiler
```

## Architecture (for engine devs)

```
┌─────────────────────────────────────────────┐
│                  Engine                      │
│  ┌─────────┐    ┌──────────────────────┐    │
│  │ profiler│    │   profiler_window    │    │
│  │   .rs   │    │        .rs           │    │
│  ├─────────┤    ├──────────────────────┤    │
│  │ Macros: │    │ ProfilerWindow:      │    │
│  │ profile_│───>│ - Collects puffin    │    │
│  │ function│    │   frame data         │    │
│  │ profile_│    │ - CPU renders bars   │    │
│  │ scope   │    │ - Handles hover      │    │
│  │         │    │ - wgpu surface       │    │
│  │ init()  │    │                      │    │
│  │ new_    │    │ track_frame_time()   │    │
│  │ frame() │    │ render()             │    │
│  └─────────┘    └──────────────────────┘    │
└─────────────────────────────────────────────┘
             │                │
             ▼                ▼
     ┌───────────────────────────────┐
     │           puffin              │
     │  GlobalProfiler → FrameView   │
     └───────────────────────────────┘
```

### Key Files

| File | Purpose |
|------|---------|
| `src/core/profiler.rs` | Macros, init, new_frame |
| `src/core/profiler_window.rs` | Windowed flamegraph viewer |
| `src/core/config/debug.rs` | `puffin_profiler` config option |
| `src/cli/commands/dev.rs` | `--profiler` CLI flag handling |

### How Data Flows

1. Game code calls `profile_function!()` / `profile_scope!("name")`
2. Puffin records scope enter/exit times in thread-local storage
3. Engine calls `profiler::new_frame()` at frame start
4. ProfilerWindow's `FrameView` sink receives completed frame data
5. `render()` collects scopes and draws CPU-rendered bars
6. Throttled to ~10 FPS for readable, stable display

### Adding New Engine Scopes

```rust
// In engine system code
use crate::profile_scope;

fn my_system(world: &mut World, dt: f32) {
    profile_scope!("my_system");
    // ... system work
}
```

## Troubleshooting

### "No profiling data detected"

1. Ensure `--profiler` flag is used or `puffin_profiler = true` in Ryo.toml
2. Add `profile_function!()` to your update/render functions
3. The profiler compiles to no-op without `--features profiler`

### Profiler window doesn't open

1. Check console for `[Profiler] Window opened` message
2. Ensure you're using `ryo2d dev --profiler` (not just `cargo run`)
3. Window may be behind main game window

### Bars are flickering

Normal - scopes with sub-millisecond duration may flicker. The display smooths values but very fast scopes are inherently unstable.

### FPS shows wrong value

The profiler shows actual wall-clock FPS (including vsync wait), not just work time. This is intentional - it reflects real frame rate.

## Performance Impact

Profiling overhead is negligible when enabled:
- ~200-400ns per `profile_scope!()` call
- Profiler window runs at 10 FPS to minimize impact
- When `profiler` feature is disabled, zero overhead

## See Also

- [Profiling Guide](../../docs/src/concepts/profiling.md) - Full user documentation
- [Debugging Guide](../../docs/src/concepts/debugging.md) - Debug overlays
- `src/core/AGENTS.md` - Core module architecture
