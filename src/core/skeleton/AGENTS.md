# AGENTS.md - Skeletal Animation Module

## Design

- `SkeletonDef` is an immutable bone-hierarchy template shared via `Arc`. `Skeleton` is the per-entity ECS component holding that `Arc` plus a mutable pose and cached world transforms.
- `SkeletalBlender` crossfades from a **frozen snapshot** of the "from" pose to the **live** "to" pose, so the outgoing animation does not keep advancing during the blend.
- `SkeletalController` = FSM + animation map + `SkeletalAnimator` + `SkeletalBlender`. `set_state` validates through the FSM, and `force_state` bypasses it.

## Coordinate System

Y-down: negative Y = up on screen, positive rotation = clockwise.

## System Integration

- `SkeletonSystem` (`src/core/systems/skeleton.rs`) processes all skeleton entities each frame. Entities with `SkeletalController` are handled exclusively by the controller path. Standalone `SkeletalAnimator` entities use the animator path. Having both on one entity is unsupported.
- **Mutate the entity's component, not a copy**: drive animation via `ctx.world.get_mut::<SkeletalController>(entity)`. The system advances time on the component stored in the world, so changes to a local or cloned controller are never seen. See `examples/demos/skeleton` for the pattern.
