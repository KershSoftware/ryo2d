# Raycast Y-Axis Camera (Pitch) Design

## Problem

The raycasting renderer has no vertical look support. The camera is locked to a horizontal plane — the horizon is always at screen center. This limits the 2.5D experience to Wolfenstein-era flat-looking gameplay.

## Approach: Horizon-Shift

Shift the horizon line vertically based on a pitch value. Rays are still cast horizontally — the vertical look is a projection trick (same technique used by Doom, Duke Nukem 3D, Dark Forces).

```
horizon = screen_h * 0.5 + pitch * screen_h
```

This requires no changes to the DDA algorithm. The shearing artifact at extreme angles is expected and authentic for 2.5D rendering. A configurable `max_pitch` clamp mitigates it.

Alternatives considered and rejected:
- **Per-column vertical ray extension** — geometrically more correct but significantly more complex shader math, slower, and overkill for 2.5D.
- **Camera matrix transform** — clean separation but causes post-processing distortion artifacts and breaks sprite depth sorting.

## Design

### Component Changes (`src/core/raycast/components.rs`)

Add two fields to `RaycastCamera`:

```rust
pub pitch: f32,       // vertical look offset in radians (0 = level)
pub max_pitch: f32,   // configurable clamp, default PI/4 (45 degrees)
```

New methods:
- `set_pitch(pitch: f32)` — clamps to `+-max_pitch`
- `pitch() -> f32` — getter
- `adjust_pitch(delta: f32)` — relative adjustment with clamping

Default values: `pitch: 0.0`, `max_pitch: PI / 4.0`.

### GPU Uniform Changes (`src/core/raycast/systems.rs`)

Replace the 2-float `_pad` in `CameraUniform` with `pitch` + 1 float pad:

```
Before: pos[2] + dir[2] + plane[2] + screen_size[2] + height + time + _pad[2] = 48 bytes
After:  pos[2] + dir[2] + plane[2] + screen_size[2] + height + time + pitch + _pad[1] = 48 bytes
```

Total size stays 48 bytes. No buffer or bind group layout changes needed.

### Wall Shader Changes (`src/core/shaders/core/raycast.wgsl`)

Camera struct: replace `_pad: vec2<f32>` with `pitch: f32` and `_pad: f32`.

Compute horizon once per pixel:

```wgsl
let horizon = screen_h * 0.5 + camera.pitch * screen_h;
```

Substitute `horizon` for `screen_h * 0.5` in five locations:
1. No-hit sky/ground split (line ~290)
2. `wall_top` calculation (line ~316)
3. `wall_bottom` calculation (line ~317)
4. Floor distance calculation (line ~378)
5. Ceiling distance denominator (line ~426)

No DDA algorithm changes.

### Sprite Shader Changes (`src/core/shaders/core/raycast_sprite.wgsl`)

Same Camera struct change. One substitution for sprite vertical positioning:

```wgsl
// v_center: add pitch offset
let v_center = screen_h * 0.5 + camera.pitch * screen_h
             + (camera.height - 0.5 - sprite.v_offset) * screen_h / transform_y;
```

### Engine Sync (`src/core/engine.rs`)

In the CameraUniform construction (~line 2263), add:

```rust
pitch: camera.pitch,
_pad: [0.0],  // was [0.0; 2]
```

### Demo Updates (`examples/demos/raycasting/main.rs`)

- Add `player_pitch: f32` field (init `0.0`)
- Capture mouse Y delta (currently discarded as `_`)
- Sync pitch to camera via `camera.set_pitch(self.player_pitch)`
- Show pitch in HUD text

### Tests (`src/core/raycast/components.rs`)

- `camera_pitch_clamp` — `set_pitch` clamps to `+-max_pitch`
- `camera_pitch_default` — `new()` creates `pitch: 0.0, max_pitch: PI/4`
- `camera_adjust_pitch` — relative adjustment clamps correctly
- `camera_custom_max_pitch` — custom `max_pitch` value works

Existing `camera_uniform_size` test in `systems.rs` continues to assert 48 bytes.

## Files Touched

| File | Change |
|------|--------|
| `src/core/raycast/components.rs` | `pitch`, `max_pitch` fields + 3 methods + 4 tests |
| `src/core/raycast/systems.rs` | `_pad: [f32; 2]` -> `pitch: f32` + `_pad: f32` |
| `src/core/shaders/core/raycast.wgsl` | Camera struct + `horizon` variable, 5 substitutions |
| `src/core/shaders/core/raycast_sprite.wgsl` | Camera struct + 1 substitution |
| `src/core/engine.rs` | `pitch` field in CameraUniform construction |
| `examples/demos/raycasting/main.rs` | `player_pitch`, mouse Y input, camera sync, HUD |

## Rebase Notes

A parallel session is adding professional lighting features to the raycasting shaders. Likely conflicts:
- `raycast.wgsl` and `raycast_sprite.wgsl` Camera struct definitions
- `systems.rs` CameraUniform struct
- `engine.rs` CameraUniform construction

All conflicts are in the Camera struct, which is small and mechanical to resolve. This work should land first, then rebase against lighting changes.

## Follow-Up (Not In Scope)

- Built-in opt-in mouse-look convenience system (maps mouse Y delta to pitch with sensitivity config)
- Any interaction with the parallel lighting feature work
