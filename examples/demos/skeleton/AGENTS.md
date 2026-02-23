# AGENTS.md - Skeleton Demo

## Overview

Demonstrates the skeletal animation system with three themed scenes, each using `SkeletalController` for FSM-driven animation with crossfade blending.

## File Structure

| File | Purpose |
|------|---------|
| `main.rs` | Shared skeleton definition, main scene orchestrator, entry point |
| `scenes/mod.rs` | Re-exports scene structs, `SceneBase` shared helper |
| `scenes/fighter.rs` | Fighter scene: idle, walk, punch, kick, block |
| `scenes/casual.rs` | Casual scene: idle, walk, wave, jump, dance |
| `scenes/action_rpg.rs` | Action RPG scene: idle, run, attack, dodge, victory |

## Shared Code (main.rs)

- `DebugBone` - marker component for debug line entities (despawned/respawned each frame)
- `create_stick_fighter()` - 22-bone humanoid skeleton definition (Y-down coords)
- `SkeletonDemoScene` - implements `Scene` trait, delegates to sub-scenes, handles Tab cycling

## Scene Pattern

Each scene wraps a `SceneBase` (in `scenes/mod.rs`) that handles:
- Entity spawning/despawning (`enter`/`exit`)
- Debug line management (`redraw_debug_lines`)
- State caching for UI (`cache_state`)
- Standard UI rendering (`render_ui`)

Each scene only provides its unique `update()` input handling logic:
1. `new(def)` - creates animations, builds `SkeletalController`, wraps in `SceneBase`
2. `update(ctx, dt)` - handles input via `ctx.world.get_mut::<SkeletalController>(entity)`, calls `base.cache_state()`

**Important:** All input handling operates directly on the entity's controller via `world.get_mut`, NOT on a local copy. The `SkeletonSystem` advances animation time on the entity's controller each frame.

## Cross-Scene Dependencies

- `CasualScene` reuses `create_fighter_idle()` from `scenes/fighter.rs` (same breathing animation)
- All scenes use `SceneBase` from `scenes/mod.rs`
- All scenes use `DebugBone` from `main.rs`

## Controls

| Scene | Movement | Actions |
|-------|----------|---------|
| Fighter | A/D: Walk | J: Punch, K: Kick, L: Block (hold) |
| Casual | A/D: Walk | 1: Wave, 2: Jump, 3: Dance (toggle) |
| Action RPG | A/D: Run | J: Attack, K: Dodge, V: Victory |

Global: Tab = next scene, Esc = exit
