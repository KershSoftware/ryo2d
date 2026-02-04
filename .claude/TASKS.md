## Phase 0: Feature Enhancements 🚀
> Focus: Polish existing systems before adding new ones. All items are quick wins (Low-Medium complexity).

### 0.1 Tweening System Enhancements ✅
**File:** `src/core/ecs/tween.rs`

- [x] **Callbacks System**
  - Added `TweenStarted` and `TweenCompleted` events via event bus
  - Engine emits events at appropriate lifecycle points
  - *Completed in commit 7c5bfde*

- [x] **Speed/Playback Rate Control**
  - Added `speed: f32` field (default 1.0)
  - `.with_speed(multiplier)` builder method
  - Delta time multiplied by speed in update
  - *Completed in commit 7c5bfde*

- [x] **Color/Alpha Animation**
  - Added `TweenProperty::Alpha { from, to }` and `Tint { from, to }` variants
  - Added `alpha` and `tint` fields to `Renderable` component
  - CPU renderer support via `draw_rotated_tinted()` with premultiplied alpha interpolation
  - Fixed black fringing artifacts on anti-aliased edges
  - *Completed in commits 372be6f, 674e7da, ad41077, dfee13b*

- [x] **Tween Chaining/Sequencing**
  - Added `TweenSequence` component for queued tweens
  - `.then()` method to chain multiple tweens
  - Auto-advances to next tween on completion
  - Demo scene 5 showcases sequences
  - *Completed in commit 7c5bfde*

### 0.2 Screen Shake & Game Juice Enhancements
**Files:** `src/core/graphics/camera.rs`, `src/core/game.rs`

- [x] **Knockback Helper**
  - Added `Knockback` component with direction, force, strength-based decay
  - `ctx.knockback(entity, direction, force)` and `ctx.knockback_with_decay()` methods
  - Accumulation mode for blending multiple knockbacks
  - `KnockbackStarted`/`KnockbackFinished` events
  - Refactored shared `ContextCore` for GameContext/SceneContext
  - *Completed in commit 7003a66*

- [x] **Squash and Stretch Utilities**
  - Added `SquashStretch` component with intensity, decay_rate, axis
  - Volume preservation (squash Y = stretch X)
  - `ctx.squash(entity, intensity)`, `ctx.squash_x()`, `ctx.squash_with_decay()`
  - `SquashStretchStarted`/`SquashStretchFinished` events
  - Demo with bouncing balls and physics
  - *Completed in commit (pending)*

- [x] **Configurable Shake Intensity Curves**
  - Added `intensity_curve: Ease` field to `CameraShake`
  - `camera.set_shake_curve(Ease::InCubic)` for different feel
  - Default `Ease::InQuad` for backwards compatibility
  - Builder method `CameraShake::new().with_intensity_curve()`
  - *Completed in commit (pending)*

### 0.3 Pathfinding Enhancements
**File:** `src/core/navigation/steering.rs`

- [x] **Steering Behaviors Module**
  - Added `steering` module with standalone steering functions
  - `separation(pos, radius, neighbors, strength)` - agent avoidance
  - `seek(pos, target, max_speed)` - move toward target
  - `flee(pos, threat, max_speed)` - move away from threat
  - `arrive(pos, target, max_speed, slow_radius)` - seek with slowdown
  - `cohesion(pos, neighbors, max_speed)` - move toward group center
  - `alignment(neighbors, max_speed)` - match group velocity
  - Games apply steering manually for full control
  - *Completed in commit (pending)*

### 0.4 Trail Renderer Enhancements
**Files:** `src/core/trail/trail.rs`, `src/core/trail/renderer.rs`

- [ ] **Texture Mapping Along Trails**
  - Add texture binding to `TrailRenderer` pipeline
  - Wire up UV coordinates already in shader
  - Add `texture: Option<Handle<Texture>>` to `Trail2D`
  - Support stretched vs. tiled modes
  - *Complexity: Medium (~4-6 hours)*

- [x] **Lifetime/TTL Auto-Removal**
  - Added `lifetime: Option<f32>` field to `Trail2D`
  - Points automatically removed when `age > lifetime`
  - `age_alpha(age)` helper for time-based fading (when position fade is off)
  - `.with_lifetime(seconds)` builder method
  - Engine Trail System auto-updates all Trail2D components
  - Two fade modes: position-based (`.with_fade(true)`) vs time-based (`.with_lifetime()`)
  - Trails demo showcasing both fade modes
  - *Completed in commit (pending)*

- [x] **Distance-Based Fade**
  - Added `fade_distance: Option<f32>` field to Trail2D
  - `.with_fade_distance(pixels)` builder method
  - `distance_alpha()` and `cumulative_distances()` methods
  - `total_length()` helper for trail length in pixels
  - Renderer uses distance-based fade when `fade_distance` is set
  - More predictable than position-based fade (consistent regardless of speed)
  - Trails demo updated with D key for distance mode
  - *Completed in commit (pending)*

### Phase 0 Summary

| Task | Complexity | Status |
|------|------------|--------|
| Tween: Callbacks | Medium | ✅ Done |
| Tween: Speed control | Low | ✅ Done |
| Tween: Color/alpha | Medium | ✅ Done |
| Tween: Chaining | Medium | ✅ Done |
| Juice: Knockback | Low-Medium | ✅ Done |
| Juice: Squash/stretch | Low-Medium | ✅ Done |
| Juice: Shake curves | Low | ✅ Done |
| Pathfinding: Steering | Medium | ✅ Done |
| Trail: Texture mapping | Medium | ⬜ Pending |
| Trail: Lifetime/TTL | Low | ✅ Done |
| Trail: Distance fade | Low-Medium | ✅ Done |
| **Progress** | | **10/11 complete** |