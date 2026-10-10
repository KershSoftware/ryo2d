# AGENTS.md - Game Systems

## Conventions

- Stateless unit structs with associated fns (`MySystem::update(world, ..)`); all state lives in the World, so systems test in isolation and can be reordered
- Skip `Inactive` entities (object pooling) in every system
- Take scaled `dt` (time_scale/hitstop already applied)
- Called from `Engine::update()` in `engine.rs`; the order is commented there. `InterpolationSystem::snapshot()` runs once per frame before the fixed-step loop, not inside `update()`

## InterpolationSystem

- Opt in per entity with `Renderable::sprite(..).interpolated()`; `PreviousTransform` is auto-added on the next snapshot
- After teleporting, call `InterpolationSystem::skip_interpolation(world, entity)` or the entity visibly slides from its old position
- Use for: fast movers (balls, bullets), player-controlled entities, scrolling backgrounds
- Skip for: static entities, short-lived particles, frequent teleporters
