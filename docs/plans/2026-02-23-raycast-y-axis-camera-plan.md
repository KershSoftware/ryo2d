# Raycast Y-Axis Camera (Pitch) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add configurable vertical look (pitch) to the 2.5D raycasting renderer using the horizon-shift technique.

**Architecture:** A `pitch` field on `RaycastCamera` is clamped to a configurable `max_pitch`, passed through the existing 48-byte `CameraUniform` (replacing padding), and used in both WGSL shaders to shift the horizon line. No DDA algorithm changes. No buffer layout changes.

**Tech Stack:** Rust (components, systems, engine sync), WGSL (GPU shaders), hecs ECS

**Design doc:** `docs/plans/2026-02-23-raycast-y-axis-camera-design.md`

**Pre-requisite:** Rebase this branch (`feat/raycasting-y-axis-camera`) against latest `main` before starting. Lighting work has landed (`43ca73b`). The `apply_lights` signature changed but Camera struct and `screen_h * 0.5` locations are unaffected — plan logic is unchanged, only line numbers shifted.

---

### Task 1: RaycastCamera pitch fields and methods

**Files:**
- Modify: `src/core/raycast/components.rs:15-63`

**Step 1: Write the failing tests**

Add these tests to the existing `#[cfg(test)] mod tests` block in `components.rs`:

```rust
#[test]
fn camera_pitch_default() {
    let cam = RaycastCamera::new(66.0);
    assert!((cam.pitch - 0.0).abs() < EPSILON);
    assert!((cam.max_pitch - PI / 4.0).abs() < EPSILON);
}

#[test]
fn camera_pitch_clamp() {
    let mut cam = RaycastCamera::new(66.0);
    cam.set_pitch(1.0); // exceeds default max_pitch of PI/4 (~0.785)
    assert!((cam.pitch - cam.max_pitch).abs() < EPSILON);
    cam.set_pitch(-1.0);
    assert!((cam.pitch - (-cam.max_pitch)).abs() < EPSILON);
}

#[test]
fn camera_adjust_pitch() {
    let mut cam = RaycastCamera::new(66.0);
    cam.adjust_pitch(0.3);
    assert!((cam.pitch - 0.3).abs() < EPSILON);
    cam.adjust_pitch(0.3);
    assert!((cam.pitch - 0.6).abs() < EPSILON);
    // Should clamp at max_pitch
    cam.adjust_pitch(1.0);
    assert!((cam.pitch - cam.max_pitch).abs() < EPSILON);
}

#[test]
fn camera_custom_max_pitch() {
    let mut cam = RaycastCamera::new(66.0);
    cam.max_pitch = PI / 6.0; // 30 degrees
    cam.set_pitch(1.0);
    assert!((cam.pitch - PI / 6.0).abs() < EPSILON);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p ryo2d --lib raycast::components::tests`
Expected: FAIL — `pitch`, `max_pitch`, `set_pitch`, `adjust_pitch` don't exist yet.

**Step 3: Add pitch fields and methods to RaycastCamera**

Add fields to the `RaycastCamera` struct (after `height`):

```rust
/// Vertical pitch offset in radians (0.0 = level, positive = look up).
pub pitch: f32,
/// Maximum allowed pitch magnitude in radians.
pub max_pitch: f32,
```

Update `new()` to initialize them:

```rust
pitch: 0.0,
max_pitch: std::f32::consts::FRAC_PI_4, // 45 degrees
```

Add methods to the `impl RaycastCamera` block:

```rust
/// Set vertical pitch in radians, clamped to `±max_pitch`.
pub fn set_pitch(&mut self, pitch: f32) {
    self.pitch = pitch.clamp(-self.max_pitch, self.max_pitch);
}

/// Get the current pitch in radians.
pub fn pitch(&self) -> f32 {
    self.pitch
}

/// Adjust pitch by `delta` radians, clamped to `±max_pitch`.
pub fn adjust_pitch(&mut self, delta: f32) {
    self.set_pitch(self.pitch + delta);
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p ryo2d --lib raycast::components::tests`
Expected: All tests PASS (including existing ones).

**Step 5: Commit**

```
git add src/core/raycast/components.rs
git commit -m "feat(raycast): add pitch and max_pitch to RaycastCamera"
```

---

### Task 2: CameraUniform pitch field

**Files:**
- Modify: `src/core/raycast/systems.rs:41-58`

**Step 1: Run the existing size test to confirm baseline**

Run: `cargo test -p ryo2d --lib raycast::systems::tests::camera_uniform_size`
Expected: PASS (48 bytes).

**Step 2: Replace `_pad: [f32; 2]` with `pitch: f32` and `_pad: f32`**

In `CameraUniform`, change:

```rust
// Before:
/// Padding to reach 48 bytes.
pub _pad: [f32; 2],

// After:
/// Vertical pitch for horizon shift.
pub pitch: f32,
/// Padding to reach 48 bytes.
pub _pad: f32,
```

**Step 3: Run the size test to confirm no breakage**

Run: `cargo test -p ryo2d --lib raycast::systems::tests::camera_uniform_size`
Expected: PASS (still 48 bytes).

**Step 4: Fix compilation**

The `_pad` field changed type from `[f32; 2]` to `f32`. Update the one place that constructs `CameraUniform`:

In `src/core/engine.rs` (~line 2263), change the construction:

```rust
// Before:
_pad: [0.0; 2],

// After:
pitch: camera.pitch,
_pad: 0.0,
```

**Step 5: Run full check**

Run: `cargo check`
Expected: No errors.

**Step 6: Commit**

```
git add src/core/raycast/systems.rs src/core/engine.rs
git commit -m "feat(raycast): add pitch to CameraUniform, sync from ECS"
```

---

### Task 3: Wall shader horizon-shift

**Files:**
- Modify: `src/core/shaders/core/raycast.wgsl`

**Step 1: Update Camera struct in the shader**

Change the Camera struct (lines 22-30):

```wgsl
// Before:
    _pad: vec2<f32>,

// After:
    pitch: f32,
    _pad: f32,
```

**Step 2: Add horizon variable after screen coordinate setup**

After `let screen_y = ...` (line 283), add:

```wgsl
let horizon = screen_h * 0.5 + camera.pitch * screen_h;
```

**Step 3: Replace `screen_h * 0.5` with `horizon` in 5 locations**

Location 1 — no-hit sky/ground split (line 380):
```wgsl
// Before:
if screen_y < screen_h * 0.5 {
// After:
if screen_y < horizon {
```

Location 2 — wall_top (line 406):
```wgsl
// Before:
let wall_top = -line_height * (ceil_h - camera.height) + screen_h * 0.5;
// After:
let wall_top = -line_height * (ceil_h - camera.height) + horizon;
```

Location 3 — wall_bottom (line 407):
```wgsl
// Before:
let wall_bottom = line_height * (camera.height - floor_h) + screen_h * 0.5;
// After:
let wall_bottom = line_height * (camera.height - floor_h) + horizon;
```

Location 4 — floor distance (line 467-468):
```wgsl
// Before:
let floor_dist = (camera.height - floor_h) * screen_h
                 / (screen_y - screen_h * 0.5);
// After:
let floor_dist = (camera.height - floor_h) * screen_h
                 / (screen_y - horizon);
```

Location 5 — ceiling denominator (line 515):
```wgsl
// Before:
let denom = screen_h * 0.5 - screen_y;
// After:
let denom = horizon - screen_y;
```

**Step 4: Run compilation check**

Run: `cargo check`
Expected: No errors (WGSL is validated at pipeline creation, but cargo check confirms Rust compiles).

**Step 5: Commit**

```
git add src/core/shaders/core/raycast.wgsl
git commit -m "feat(raycast): horizon-shift pitch support in wall shader"
```

---

### Task 4: Sprite shader horizon-shift

**Files:**
- Modify: `src/core/shaders/core/raycast_sprite.wgsl`

**Step 1: Update Camera struct**

Same change as the wall shader (lines 11-19):

```wgsl
// Before:
    _pad: vec2<f32>,

// After:
    pitch: f32,
    _pad: f32,
```

**Step 2: Add pitch offset to sprite vertical center**

Change the v_center calculation (line ~84):

```wgsl
// Before:
let v_center = screen_h * 0.5 + (camera.height - 0.5 - sprite.v_offset) * screen_h / transform_y;

// After:
let v_center = screen_h * 0.5 + camera.pitch * screen_h + (camera.height - 0.5 - sprite.v_offset) * screen_h / transform_y;
```

**Step 3: Run compilation check**

Run: `cargo check`
Expected: No errors.

**Step 4: Commit**

```
git add src/core/shaders/core/raycast_sprite.wgsl
git commit -m "feat(raycast): pitch support in sprite billboard shader"
```

---

### Task 5: Demo — mouse pitch input

**Files:**
- Modify: `examples/demos/raycasting/main.rs`

**Step 1: Add `player_pitch` field to `RaycastDemo` struct**

After `player_angle: f32,` (line 25), add:

```rust
player_pitch: f32,
```

Initialize it in `new()`:

```rust
player_pitch: 0.0,
```

**Step 2: Capture mouse Y delta**

Change line 258:

```rust
// Before:
let (mouse_dx, _) = ctx.input.mouse_delta();

// After:
let (mouse_dx, mouse_dy) = ctx.input.mouse_delta();
self.player_pitch = (self.player_pitch - mouse_dy * 0.003).clamp(-0.7, 0.7);
```

Note: clamping to `±0.7` radians (~40°) at the demo level. The camera's `max_pitch` (π/4 ≈ 0.785) provides the authoritative clamp.

**Step 3: Sync pitch to camera**

After `camera.set_angle(self.player_angle);` (line 399), add:

```rust
camera.set_pitch(self.player_pitch);
```

**Step 4: Update HUD display**

Change the position format string (~line 437):

```rust
// Before:
&format!(
    "Pos: ({:.1}, {:.1})  Angle: {:.1}°",
    self.player_x,
    self.player_y,
    self.player_angle.to_degrees()
),

// After:
&format!(
    "Pos: ({:.1}, {:.1})  Angle: {:.1}°  Pitch: {:.1}°",
    self.player_x,
    self.player_y,
    self.player_angle.to_degrees(),
    self.player_pitch.to_degrees()
),
```

**Step 5: Run compilation check**

Run: `cargo check --example raycasting`
Expected: No errors.

**Step 6: Visual test**

Run: `./run raycasting` (or `./run.ps1 raycasting` on Windows)
Expected: Moving the mouse up/down shifts the horizon. Walls, floors, ceilings, and sprites all move together consistently. Looking straight ahead (no mouse movement) looks identical to before.

**Step 7: Commit**

```
git add examples/demos/raycasting/main.rs
git commit -m "feat(raycast): add mouse pitch to raycasting demo"
```

---

### Task 6: Final validation

**Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

**Step 3: Run the demo again and verify edge cases**

- Look all the way up — floor disappears, ceiling fills screen
- Look all the way down — ceiling disappears, floor fills screen
- Look level — identical to pre-pitch behavior
- Walk through a door while looking up/down — door renders correctly
- Sprites stay grounded while pitching — no floating/sinking

**Step 4: Final commit (if any fixups needed)**

Only if clippy or tests required changes.

---

### Post-Implementation: Verify Lighting Integration

Lighting work landed on `main` before implementation (`43ca73b`). After completing all tasks:

1. Visual test the demo to confirm lighting + pitch work together
2. Verify shadow rays and smooth falloff render correctly when pitched up/down
3. Confirm dark room torches illuminate properly at all pitch angles
