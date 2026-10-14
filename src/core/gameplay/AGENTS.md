# AGENTS.md - Gameplay Module

Gameplay on top of physics: movement presets and the input that drives them, trigger behaviours and knockback. Later pieces are added here as they land; this file lists what exists.

## Dependency Rule

`gameplay` depends on `physics`, never the reverse, and names no Rapier type (it uses `Physics` queries, `CharacterController`, `RigidBody`, messages). Components that physics must read stay in `src/core/ecs/` (`Knockback`).

## The `Movement` Set

`register_gameplay_systems` (`mod.rs`) adds one chain to `FixedSet::Movement`, which runs after the scene's `update` and `FixedSet::Game` and before `FixedSet::Physics`. Systems join the chain in this order as they are written: `place_respawn_points`, `warn_untriggered`, `collect_pickups`, `respawn_pickups`, `apply_kill_zones`, `apply_checkpoints`, `read_move_bindings`, `update_knockback`, `platformer_movement`, `top_down_movement`. Today only `update_knockback` is in it. Every gameplay system's resources are `Time`, `Input` and messages, so a world with those three (`tests/integration/support.rs::gameplay_world`) runs them without a simulation.

Invalid input is logged once per entity with a `Local<EntityHashSet>` and `crate::engine_warn!(module: "Gameplay", ...)`; nothing panics.

## Knockback

`update_knockback` (`knockback.rs`) decays every active `Knockback` and reports `KnockbackStarted` / `KnockbackFinished`. What the push does depends on the target:

| Target | Effect |
|---|---|
| `CharacterController` | Nothing here: `move_characters` (physics) passes the push as the sweep's `extra` velocity and clips it against every surface the move touched (`Knockback::clip`), so a wall absorbs the part running into it and a head-on push stops |
| Dynamic body | One change in velocity when the push starts (`direction * force`), whatever the mass; physics slows it. Nothing is taken back |
| Kinematic body without a controller, fixed body | Ignored, warned once per entity |
| No body | `Transform` moves by the current knockback velocity |

## Tests

`test_util.rs`: `world(hz)` is the physics test world plus `Input` and the gameplay systems; `player(world, x, y)` spawns a 20x40 platformer character; `at`, `no_gravity` and `run` come from the physics helpers. Single-system tests use `systems::test_util` (`world`, `run`, `messages`).
