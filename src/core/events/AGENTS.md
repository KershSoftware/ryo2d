# AGENTS.md - Events Module

## Event Lifetime

Events last **one fixed-timestep update**, not one rendered frame:

1. `Engine::update` calls `events.clear()` first, then emits `CollisionStarted`/`CollisionStopped` from the previous physics step
2. Scenes and systems emit and read during that update
3. Fixed updates can run several times per rendered frame, and each one clears the bus
4. Scene `Switch`/`Reset` also clears events (`SceneCleanup::EVENTS`, on by default)

## Built-in Events

The engine only auto-emits collision events (engine.rs), Tween*/Knockback*/SquashStretch* events (from `core/systems`). Network (`ClientConnected`, ...), asset (`AssetReloaded`, `AssetLoadFailed`), inventory (`Item*`, `InventoryChanged`), procgen (`Generation*`, `Region*`) and state machine `StateChanged<S>` types are **never auto-emitted**. Game code must emit them.

## Integration Points

- `Events` is owned by `Engine` and exposed to scenes as `SceneContext.events`
- Engine systems (tween, knockback, squash_stretch) receive `&mut Events` from `Engine::update`
- `render_ui` gets `&mut UiContext` only, so it has no event access

## Performance

`emit()` is a Vec push, and `clear()` keeps each channel's capacity, so no allocations occur after warmup. Preserve this in hot paths.

## Notes for AI Agents

1. **Events are per-update** - Don't expect events to persist across updates
2. **Multiple reads OK** - Reading doesn't consume events
3. **Order matters for emit** - Events are read in emit order
