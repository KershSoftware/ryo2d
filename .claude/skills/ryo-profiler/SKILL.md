---
name: ryo-profiler
description: Profiling Ryo2D games and the engine - profile_scope! and profile_function! (puffin, with the engine's profiler feature, recorded while scopes are on), ryo2d dev -p and the profiler window (F10), the engine's own scopes, the frame timing overlay and FrameCost, and performance analysis.
metadata:
  version: 1.3.0
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

A second window opens showing real-time flamegraphs. `-p` builds the engine with its `profiler` feature (`cargo run --features ryo2d/profiler`), so the game needs no feature of its own. The profiler is a developer tool: a game built with `ryo2d build` has none (see A shipped build, below).

## Profiler Window Features

The built-in profiler window (`windows/profiler_window.rs`) provides:

- **Real-time flamegraph** - Color-coded scope bars showing time spent
- **FPS and frame time display** - Shows actual FPS and work time per frame
- **Hover tooltips** - Mouse over any bar to see scope name and duration
- **Auto-instrumentation hints** - Shows setup instructions if no scopes detected
- **Scope persistence** - Bars fade smoothly rather than flickering

## The Frame Timing Overlay and `FrameCost`

F1 cycles the engine's overlays (FPS, stats, frame timing; also `debug:timing` in the console). The frame timing overlay splits a frame's CPU time into update (the UI pass and the fixed steps: scenes, systems, physics), render (gathering what the frame draws, building and uploading it), present (running the render passes and presenting) and the total. The three do not add up to the total: the frame's other phases (`PreUpdate` with the asset pump and tile map spawning, `Update`, `PostUpdate` with transform propagation, and `First` and `Last`) count only in the total, so a cost there, such as a big map's children being propagated, shows as total less the three. Present and total leave out the wait for the window's next image, which vsync paces, so they show the work, not the display's rate; the FPS counter comes from the wall clock and does count the waits.

A game's own readout reads the same smoothed numbers from `FrameCost`, a resource the engine writes at the end of every frame, windowed or headless (read it; the engine puts it back each frame):

```rust
fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
  if let Some(cost) = ctx.resource::<FrameCost>() {
    let line = format!(
      "update {:.2} ms  render {:.2} ms  present {:.2} ms  total {:.2} ms  {:.0} fps",
      cost.update_ms(),
      cost.render_ms(),
      cost.present_ms(),
      cost.total_ms(),
      cost.fps(),
    );
    ui.draw_text(&line, 8.0, 8.0, Color::WHITE);
  }
  SceneAction::Continue
}
```

- The values are smoothed over about 50 frames (a plain mean until then), so a change shows within a second at 60 fps; `frames()` counts the frames measured, and `fps()` is 0 until the second frame.
- Compare a feature on and off on the same machine to see what it costs: the tilemap demo's Big map page shows the map's cost this way, the frame with the map against the frame without it, with the rest of the total (total less update, render and present) as "other".
- `update_ms()` is the UI pass and the fixed steps alone; the asset pump, map spawning and transform propagation are in `total_ms()` only.
- The raw measurement of each frame is `ryo2d::core::diagnostics::FrameTimings { update, physics, render, present, wait, total }` (`Duration`s), what the engine smooths; its `wait` is the time spent waiting for the window's next image, which `present` and `total` leave out.

## Instrumenting Code

The macros are in the prelude:

```rust
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

A scope is recorded only when both hold:

- the build has the engine's `profiler` feature (`ryo2d dev -p`, `ryo2d dev` with `[debug] enable_profiler = true`, or `cargo run --features ryo2d/profiler`). Without it the macros compile to nothing, in the game and in the engine;
- scopes are on: while the profiler window is open, or for the whole run with `[debug] enable_profiler = true`. Closing the window turns them off again unless `enable_profiler` is set.

So with the feature built in and the window closed, a scope costs a check and records nothing.

## Engine Scopes

The engine's own scopes, recorded under the same conditions. The `cpu:` and `gpu:` prefixes colour the bars; a game's scopes have no prefix.

| Scope | What it measures |
|-------|-----------------|
| `cpu:update` | The UI pass and every fixed step of the frame (`Scene::update`, juice, scene systems, physics, propagation) |
| `cpu:physics` | One Rapier step (`step_physics`) |
| `cpu:particles` | One step of the engine's particle emitters (the game-time step, and the real-time step of unscaled emitters) |
| `cpu:render` | `Scene::render` hooks and collecting the world (entities, trails, lights, particles) into the GPU draw list |
| `cpu:render:particles` | Inside `cpu:render`: collecting the particles the cameras see |
| `cpu:render:submit` | Submitting the frame's GPU work |
| `cpu:atlas:spread texture`, `cpu:atlas:spread first band` | Making an image's own texture, and writing its first band, when a big image is spread over frames |
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

This builds the engine with its `profiler` feature (`--features ryo2d/profiler`) and opens the profiler window at launch (`RYO_PROFILER_AUTO_OPEN=1`). It names the engine's package, `ryo2d`: a game that renames its `ryo2d` dependency in `Cargo.toml` cannot use `-p`, and runs `cargo run --features <its name>/profiler` instead.

### Via Ryo.toml (recommended for regular development)
```toml
[debug]
enable_profiler = true
```

`ryo2d dev` then builds with the `profiler` feature too, and scopes are on from the start, but the window does not open by itself. Press **F10** to open it when needed (F10 works while `[debug] dev_window_keys` is on, its default in a build with the tools). `cargo run` alone does not add the feature: pass `--features ryo2d/profiler`.

### A shipped build

The `profiler` feature brings the developer tools with it. `ryo2d build` adds neither, so a shipped game records no scopes, has no profiler window and no F10, whatever Ryo.toml says; the macros compile to nothing.

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
| `src/core/profiler.rs` | Macros, `init` (scopes on for `enable_profiler`), `new_frame` |
| `src/core/windows/profiler_window.rs` | Windowed flamegraph viewer |
| `src/core/diagnostics/flags.rs` | `DebugConfig::enable_profiler`, the runtime switch (set from `[debug] enable_profiler` in Ryo.toml) |
| `src/cli/commands/dev.rs` | `--profiler` CLI flag handling |

### How Data Flows

1. Game code calls `profile_function!()` / `profile_scope!("name")`
2. While scopes are on (the window holds them on while open; `enable_profiler` for the whole run), puffin records scope enter/exit times in thread-local storage
3. Engine calls `profiler::new_frame()` at frame start
4. ProfilerWindow's `FrameView` sink receives completed frame data
5. `render()` collects scopes and draws CPU-rendered bars
6. Throttled to ~10 FPS for readable, stable display

### Adding New Engine Scopes

```rust,ignore
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
3. The profiler compiles to no-op without the engine's `profiler` feature

### Profiler window doesn't auto-open

If using `enable_profiler = true` in Ryo.toml, the profiler window doesn't auto-open by design. Press **F10** to open it when needed.

For auto-open behavior, use `ryo2d dev --profiler` instead.

### Profiler window doesn't open at all

1. Look in the terminal or the log for `ryo2d::core::windows::profiler_window: Window opened`
2. F10 does nothing without the engine's `profiler` feature: run through `ryo2d dev` (with `-p` or `enable_profiler = true`), or `cargo run --features ryo2d/profiler`
3. F10 needs `[debug] dev_window_keys` on (its default in a build with the tools)
4. Window may be behind main game window

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
