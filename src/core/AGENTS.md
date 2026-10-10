# AGENTS.md - Core Engine Modules

Most subsystems have their own AGENTS.md (see root table) or a skill in `.claude/skills/`. This file covers cross-cutting design and root-level files (`engine.rs`, `scene.rs`, `game.rs`, `timing.rs`, `cursor.rs`).

## Design Philosophy

- **Engine owns the loop** - user code never calls render/update directly; the `Scene` trait provides callbacks the engine invokes
- **ECS for entities** - built-in components (`Transform`, `Velocity`, `Renderable`, `RigidBody`, `Collider`); users add their own
- **Assets via handles** - loaded through `AssetManager`, shared via `Arc`-backed handles (`SpriteHandle`)
- **Minimal public surface** - expose only what users need via `lib.rs` `prelude`; internals stay private

## Time Scale & Hitstop

`TimeControl` (`game.rs`) is shared with `SceneContext`; `TimeState` (`timing.rs`) applies it.

- `scaled_fixed_dt()` = `fixed_dt * time_scale`, or 0 during hitstop (hitstop wins over time_scale)
- Scaled dt drives scene `update()` and engine systems (animation, tween, skeleton, knockback, squash/stretch, trail)
- **Rapier ignores it** - each fixed tick steps with its own `IntegrationParameters` dt, so physics keeps running during slow-mo/hitstop
- Real dt: camera shake, console, hitstop decay (after all fixed ticks)
- Scenes get both via `ctx.time` (`TimeInfo.scaled_dt` / `real_dt`)

## Cursor (`cursor.rs`)

- Deferred requests avoid borrowing the window during scene callbacks: `ctx.set_cursor*()` / `show_cursor()` / `hide_cursor()` write `Engine::cursor_request`, applied in `about_to_wait()` via `apply_cursor_request()`
- Demo: `examples/demos/cursor/`

## Related Skills

- Profiler (`profiler.rs`, `windows/profiler_window.rs`): skill `ryo-profiler`
- Developer console (`console/`): skill `ryo-console`
