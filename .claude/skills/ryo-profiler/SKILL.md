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

The built-in profiler window (`windows/profiler_window.rs`) provides:

- **Real-time flamegraph** - Color-coded scope bars showing time spent
- **FPS and frame time display** - Shows actual FPS and work time per frame
- **Hover tooltips** - Mouse over any bar to see scope name and duration
- **Auto-instrumentation hints** - Shows setup instructions if no scopes detected
- **Scope persistence** - Bars fade smoothly rather than flickering

## Instrumenting Code

```rust
use ryo2d::prelude::*;

fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    profile_function!();  // Profile entire function

    {
        profile_scope!("physics");
        // physics work...
    }

    {
        profile_scope!("ai_update");
        // AI work...
    }

    SceneAction::Continue
}
```

When `profiler` feature is disabled (default), macros compile to nothing.

## Pre-Instrumented Engine Scopes

These engine scopes are always recorded:

| Scope | What it measures |
|-------|-----------------|
| `cpu:update` | The UI pass and every fixed step of the frame (`Scene::update`, juice, scene systems, physics, propagation) |
| `cpu:physics` | One Rapier step (`step_physics`) |
| `cpu:render` | `Scene::render` hooks and collecting the world (entities, trails, lights) into the GPU draw list |
| `gpu:present` | Running the render pipeline and presenting |
| `cpu:scene_push` | Scene push transitions |
| `cpu:scene_switch` | Scene switch transitions |

## Configuration

### Via CLI with auto-open (recommended for quick profiling)
```bash
ryo2d dev --profiler
# or shorthand:
ryo2d dev -p
```

This compiles with the profiler feature and auto-opens the profiler window on launch.

### Via Ryo.toml (recommended for regular development)
```toml
[debug]
enable_profiler = true
```

This compiles with the profiler feature but doesn't auto-open. Press **F10** to open the profiler window when needed (F10 requires `dev_window_keys`, which defaults to on in debug builds only). This is ideal for development where you only occasionally need the profiler.

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
| `src/core/windows/profiler_window.rs` | Windowed flamegraph viewer |
| `src/core/config/debug.rs` | `enable_profiler` config option |
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
// In an engine system
pub fn my_system(time: Res<Time>, mut query: Query<&mut Transform>) {
  crate::profile_scope!("cpu:my_system");
  // ... system work
}
```

The `cpu:` / `gpu:` prefix picks the bar colour in the profiler window.

## Troubleshooting

### "No profiling data detected"

1. Ensure `--profiler` flag is used or `enable_profiler = true` in Ryo.toml
2. Add `profile_function!()` to your update/render functions
3. The profiler compiles to no-op without the profiler feature

### Profiler window doesn't auto-open

If using `enable_profiler = true` in Ryo.toml, the profiler window doesn't auto-open by design. Press **F10** to open it when needed.

For auto-open behavior, use `ryo2d dev --profiler` instead.

### Profiler window doesn't open at all

1. Check console for `[Profiler] Window opened` message
2. If using Ryo.toml config, ensure the project has the profiler feature in Cargo.toml
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

- `src/core/AGENTS.md` - Core module architecture
