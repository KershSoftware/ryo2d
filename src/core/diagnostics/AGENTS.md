# AGENTS.md - Diagnostics Module

The engine's own debug overlays and frame timings. Not configuration: the Ryo.toml and builder settings are resolved in `config/`, and `EngineBuilder::resolve_in` copies them into the runtime `DebugConfig` here.

| File | What |
|------|------|
| `mod.rs` | Module docs (debug options, F-keys, profiling workflow) and the re-exports |
| `flags.rs` | The runtime `DebugConfig` (the switches the engine reads each frame; its console and dev window keys default on in a build with the tools, `dev_tools`, and `switch_tools_off` turns every tool off in one without) |
| `timing.rs` | `FrameTimings` (per-phase measurements), `SmoothedFrameTimings` (a plain mean until 50 frames, then an exponential average), `FrameCost` (prelude; the world resource `publish_frame_cost` writes each frame) and `time_block` |
| `overlays.rs` | `OverlayMode` (F1 cycle), `DebugStats` and `SmoothedDebugStats`, `colors`, the collider, velocity and origin shapes (`overlay_shapes`) and the text rows (`render_fps_counter`, `render_frame_timing`, `render_stats`) |

## Who draws

- `engine/frame.rs` draws the text overlays into the UI layer each frame (`render_*`, stacked from the bottom-left by `overlay_stack_top`); `ui/debug.rs` stacks the `debug:ui-rebuilds` panels above them with the same `row_pitch` and `overlay_label`.
- `engine/extract.rs` draws the shapes into `DebugDraw` through `overlay_shapes`; the physics F2 overlay (`physics/debug.rs`) and the skeleton overlay (`skeleton/debug.rs`, `debug:skeletons`, `DebugConfig::show_skeletons`) share `colors`. The skeleton overlay is drawn where the frame draws each skeleton, so `overlay_shapes` takes the frame's interpolation fraction.
- Only `DebugConfig`, `DebugStats`, `SmoothedDebugStats`, `OverlayMode`, the timing types, `time_block` and `colors` are public; the drawing functions are `pub(crate)`. `FrameCost` is the only item in the prelude.
- The text rows are monospace and share one style (`overlay_style`); a new row goes through `overlay_label` so the stack keeps its spacing.
