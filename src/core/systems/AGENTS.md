# AGENTS.md - Engine Systems

The engine's built-in systems are ordinary schedule systems: functions taking `Query`, `Res`, `ResMut`, `MessageWriter` and friends, added to a phase and a set. The phases and sets are described in `src/core/ecs/AGENTS.md`.

| File | System | Where |
|------|--------|-------|
| `animation.rs` | `animate_sprites` | `FixedSet::Juice` |
| `tween.rs` | `update_tweens`, `update_tween_sequences`, `update_property_tweens` | `FixedSet::Juice` |
| `knockback.rs` | `update_knockback` | `FixedSet::Juice` |
| `squash_stretch.rs` | `update_squash_stretch` | `FixedSet::Juice` |
| `skeleton.rs` | `update_skeletons` | `FixedSet::Juice` |
| `physics.rs` | `register_new_bodies`, `unregister_removed_bodies`, `sync_to_physics`, `step_physics`, `sync_from_physics`, `emit_collision_messages` | `FixedSet::Physics` (the first two also in `Last`) |
| `trail.rs` | `record_trail_points` | `FixedSet::Trails` |

The juice systems run chained in the order above (`register_juice_systems`); physics registers through `register_physics_systems`. Everything else the engine schedules (clocks, input, messages, assets, propagation, camera shake, effects, tracker clearing, the scene systems) is added in `register_engine_systems` (`engine/mod.rs`).

## Conventions

- **Fixed-step systems integrate with `time.fixed.timestep()`** (`Res<Time>`). The step is constant; slow motion, pause and hitstop change how many steps run, not their length. Per-frame systems that must keep running through pause use `time.real.delta()`.
- **No filter for pooled entities**: queries skip `Disabled` entities by default. Ask with `Allow<Disabled>` only when the system must see them (physics' unregister bookkeeping does).
- **Write only real changes**: assign a component only when the value changed (or use `set_if_neq`), so change detection downstream (propagation, physics sync) sees real changes only. The tween system writes `Transform` only for tweens with transform targets; animation reassigns the sprite only when the frame changed.
- **Report through messages**: a system that has something to tell games writes an engine message type (`core/messages/builtin.rs`) with `MessageWriter`; define a type only when a system writes it.
- **World-level helpers stay in the component's module**: `Knockback::apply(world, entity, dir, force)` and `SquashStretch::apply(world, entity, intensity)` start an effect; the system only advances it.

## Adding a System

1. Write it as a function over queries and resources in this folder (or beside its module), with unit tests that run it through a `Schedule` (`systems::test_util`: `world(hz)`, `run(world, system)`, `messages::<T>(world)`).
2. Pick the set: per fixed step, before the scene reacts (`FixedSet::Juice`), after physics (`FixedSet::Transforms` is propagation: add a set or order with `.after(..)` rather than putting game logic there), or per frame (`PostUpdateSet::Extract` for render preparation). A system that reads `GlobalTransform` after moving things in the same step belongs after `FixedSet::Transforms`.
3. Register it in `register_juice_systems`, `register_physics_systems` or `register_engine_systems`, with `.in_set(..)` and `.chain()` / `.before()` / `.after()` for order inside the set.
4. Export it from `systems/mod.rs` if games or tests need to name it.

Games add their own systems with `EngineBuilder::add_systems` or `ctx.add_systems` (see `ryo-game-dev`), never here.
