# AGENTS.md - Skeletal Animation Module

## Design

- `SkeletonDef` is an immutable bone-hierarchy template shared via `Arc`. `Skeleton` is the per-entity ECS component holding that `Arc` plus a mutable pose and cached world transforms.
- `SkeletalBlender` crossfades from a **frozen snapshot** of the "from" pose to the **live** "to" pose, so the outgoing animation does not keep advancing during the blend.
- `SkeletalController` = FSM + animation map + `SkeletalAnimator` + `SkeletalBlender`. `set_state` validates through the FSM, and `force_state` bypasses it.

## Debug Drawing

`draw_skeleton(debug, def, transforms, bone_color, joint_color, width, joint_radius)` draws the bones and joints into `DebugDraw` (`ctx.debug_draw()`), so they draw on top of the world, `width` screen pixels wide at any zoom. Call it every update.

## Coordinate System

Y-down: negative Y = up on screen, positive rotation = clockwise.

## System Integration

- `update_skeletons` (`src/core/systems/skeleton.rs`) runs every fixed step in `FixedSet::Juice`, after squash and stretch: it advances each skeleton's controller (or, without one, its standalone `SkeletalAnimator`), writes the sampled pose, and places the bones at the entity's `GlobalTransform` (last step's propagated position). A controller wins over an animator on the same entity.
- **Mutate the entity's component, not a copy**: drive animation via `ctx.world.get_mut::<SkeletalController>(entity)`. The system advances time on the component stored in the world, so changes to a local or cloned controller are never seen. See `examples/demos/skeleton` for the pattern.
