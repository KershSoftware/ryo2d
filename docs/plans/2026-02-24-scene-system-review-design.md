# Scene System Review — Design Document

## Context

A developer with Phaser 3 experience reviewed the Ryo2D scene system and identified concerns about architectural similarity to Phaser, specifically around data passing between scenes and the boundary between scene-owned and global state.

This document analyzes the current scene system against that critique, compares it to other engines, and recommends targeted improvements.

## Current Architecture

### Scene Stack Model

Ryo2D uses a stack-based scene manager with four transition operations:

- **Push**: Add scene on top, pause current (modal overlays, pause menus)
- **Pop**: Remove top scene, resume below (return from modal)
- **Switch**: Replace top scene with cleanup (menu → game)
- **Reset**: Clear stack, start fresh (back to title screen)

Each operation has a `*With(Transition)` variant for visual effects (fade, slide).

### Data Passing Mechanisms

Three distinct mechanisms exist, each solving a different problem:

**1. Constructor parameters** — Scene owns the data.
```rust
SceneAction::Switch(Box::new(GameOverScene::new(score, high_score, is_new)))
```
Best for: One-time initialization data specific to the target scene.

**2. PopReturn** — Type-erased data flows up the stack (child → parent).
```rust
// Child returns data
SceneAction::PopReturn(Box::new(selected_level as u32))

// Parent receives in on_resume
fn on_resume(&mut self, _ctx: &mut SceneContext, returned: Option<Box<dyn Any>>) {
    if let Some(data) = returned {
        if let Ok(level) = data.downcast::<u32>() { ... }
    }
}
```
Best for: Modal/dialog results — level selection, confirmation dialogs, character pickers.

**3. Resources** — Typed map persisting across all scene transitions.
```rust
ctx.resources.insert(GameProgress { level: 3, score: 1000 });
// Any scene:
let progress = ctx.resources.get::<GameProgress>().unwrap();
```
Best for: Data that outlives individual scenes — save state, settings, player inventory.

### Scene Cleanup

`SceneCleanup` bitflags control automatic state reset on Switch/Reset:
- DEFAULT: world, camera, effects, lighting, time, events (preserves music and Resources)
- Customizable per-scene via `cleanup_on_exit()` override

### Lifecycle

`on_enter → update/render → on_pause → on_resume(returned) → on_exit`

Transparent scenes render below, `updates_below()` controls background updates.

## Analysis: Phaser 3 Comparison

### Where the Critique Was Valid (Pre-Overhaul)

Before the scene system overhaul (which removed the Game trait and added Resources + PopReturn), data passing was limited to constructor parameters — functionally identical to Phaser's `init(data)` bag.

### Where the Critique No Longer Applies

| Concern | Phaser 3 | Ryo2D (current) |
|---------|----------|-----------------|
| Scene-to-scene data | Untyped `data` JS object | 3 typed mechanisms |
| Global shared state | `this.scene.get()` — direct scene coupling | `Resources` — typed, decoupled |
| Cleanup boundary | Manual, error-prone | `SceneCleanup` bitflags |
| Stack model | Parallel scenes, all run simultaneously | True stack with controlled transparency |
| Lifecycle | Vague start/stop with race conditions | Clear 5-phase lifecycle |

### Broader Engine Comparison

| Engine | Scene Model | Data Passing | Global State |
|--------|------------|-------------|-------------|
| Phaser 3 | Parallel registry | `init(data)` untyped | `scene.get('Other')` coupling |
| Godot 4 | Node tree, `change_scene()` | Autoloads (singletons), signals | Autoloads persist across scenes |
| Bevy | No scenes — States enum filters systems | Everything in ECS World | Resources are global |
| Macroquad | State stack (push/pop) | Constructor params, shared Context | Manual global state |
| **Ryo2D** | Scene stack (push/pop/switch/reset) | Constructors, PopReturn, Resources | Resources map (typed, persistent) |

Ryo2D's model is closest to Macroquad/Amethyst with the Resources map elevating it above the "just constructor params" tier. The Resources system is analogous to Godot's Autoloads or Bevy's `Res<T>`.

## Identified Gaps

### Tier 1: Real Gaps

**A. PopReturn downcasting is unguided**

The downcast pattern requires nested `if let` with silent failure on type mismatch. No compile-time assistance.

Recommendation: Add a helper function.
```rust
pub fn downcast_return<T: Any>(returned: Option<Box<dyn Any>>) -> Option<T> {
    returned.and_then(|data| data.downcast::<T>().ok().map(|b| *b))
}
```

**B. No data-carrying Push/Switch/Reset variants**

Passing data to a new scene requires embedding it in the constructor. This works for simple cases but can become unwieldy when scenes need many parameters or when intermediate scenes must forward data they don't use.

Recommendation: Add `SwitchData`/`PushData`/`ResetData` variants that carry `Box<dyn Any>` and deliver it to `on_enter`. Low priority — constructor pattern is idiomatic Rust and works well in practice. Only implement if real pain is felt.

### Tier 2: Ergonomic Polish

**C. Convenience methods on SceneContext for Resources**

Reduce `ctx.resources.get::<T>()` to `ctx.resource::<T>()`.
```rust
impl SceneContext {
    pub fn resource<T: Any>(&self) -> Option<&T>
    pub fn resource_mut<T: Any>(&mut self) -> Option<&mut T>
    pub fn resource_or_default<T: Any + Default>(&mut self) -> &mut T
}
```

**D. Document the data-passing decision tree**

Add to scene documentation:
- Passing config to a new scene? → Constructor params
- Returning a result from a modal? → PopReturn
- Sharing state across the game? → Resources

### Tier 3: Skip (Over-Engineering)

- Scene message bus — `Events` system already handles decoupled communication
- Scene references (`ctx.get_scene::<T>()`) — Phaser's worst anti-pattern
- Typed scene transitions (generic `SceneAction<T>`) — marginal benefit, complexity cost
- Scene-scoped resources — premature until real pain is felt

## Implementation Priority

| Priority | Change | Effort |
|----------|--------|--------|
| High | Add `downcast_return<T>()` helper | ~10 lines |
| Medium | Add `ctx.resource::<T>()` convenience methods | ~20 lines |
| Medium | Document data-passing decision tree | Docs only |
| Low | Add data-carrying Push/Switch/Reset variants | ~50 lines, API change |

## Conclusion

The current scene system is architecturally sound and meaningfully different from Phaser 3. The friend's critique was likely based on a pre-overhaul version that lacked Resources and PopReturn. The recommended changes are ergonomic improvements, not architectural fixes.
