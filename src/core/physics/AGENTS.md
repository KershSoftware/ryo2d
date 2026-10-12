# AGENTS.md - Physics Module

Rapier2D behind `RapierPhysicsWorld` (a resource), driven by the systems in `src/core/systems/physics.rs`. Usage is covered by the `ryo-physics` skill.

## Coordinate System

Rapier uses meters, Ryo2D uses pixels:

- **100 pixels = 1 meter** (`PIXELS_PER_METER`, a private const in `rapier.rs`)
- Conversion happens automatically in `RapierPhysicsWorld`
- Gravity is specified in pixels/s² via the `PhysicsConfig` resource (default `(0.0, 980.0)`, +Y = down)

## Geometry

- A `Collider` is centred on the entity's origin plus `Collider.offset`, unscaled, as Rapier simulates it. `Collider::world_center(position)` and `world_bounds(position)` give that geometry; particles, navigation and the collider overlay use them, so everything agrees with physics.
- A sprite drawn over a collider uses the default `Anchor::Center`; a top-left-anchored sprite needs the collider offset by half its size.
- `GroupedCollider` (composite tiles, `graphics/tile.rs`) keeps `offset_x` / `offset_y` as the box's top-left relative to the origin, the way composite parts are laid out; its centre is `offset + size / 2`.
- Rotation is locked: bodies never rotate and `Transform.rotation` is not synced.

## The Step

`FixedSet::Physics`, every fixed step while `PhysicsPaused` is false (the scene manager sets it from the top scene's `is_physics_paused` and the stack shape), chained:

1. `unregister_removed_bodies`: bodies whose `RigidBody` went away (despawn included) and entities that were just disabled.
2. `drop_parented_bodies`: a body that gained a parent leaves the simulation, logged once.
3. `register_restored_bodies`: bodies whose entity was re-enabled or detached from its parent.
4. `register_new_bodies`: root entities that gained `RigidBody`, `Collider` or `GroupedCollider` (`Added<..>`); an edited `RigidBody` on a registered body is applied in place (`apply_body_settings`: body type, gravity scale, drag, CCD; only differing values are written, so contacts and sleep survive).
5. `sync_to_physics`: a changed `Transform` teleports the body (a kinematic body moves there over the step); a changed `Velocity` sets its velocity.
6. `step_physics`: `dt` is `time.fixed.timestep()`, so physics follows game speed, pause and hitstop.
7. `sync_from_physics`: awake dynamic bodies write position and velocity back.
8. `emit_collision_messages`: Rapier's contacts drained into `CollisionStarted` / `CollisionStopped`.

## Notes for AI Agents

1. **Sync is change-detected and compared by value.** Physics' write-back is an ordinary change (propagation must see bodies move), so `sync_to_physics` cannot tell it from a game write by change detection alone: it pushes a `Transform` or `Velocity` only when it differs from the body's. A body nobody touched stays asleep.
2. **Removal bookkeeping also runs in `Last`** every frame, before the trackers are cleared: `unregister_removed_bodies` and `register_restored_bodies` only. Removal events survive two tracker clears, and at render rates above the fixed rate no fixed step may run in between; without it a despawned body would stay in Rapier. Added and edited bodies are tick-based and wait for the next fixed step (running them in `Last` too rebuilt bodies the step had already simulated).
3. **Bodies must be root entities.** Physics reads and writes the local `Transform` as world space, so a `RigidBody` on an entity with `ChildOf` is not simulated: it is logged once and left out, and detaching the entity brings it back.
4. **An entity is simulated only with `RigidBody` + `Transform` + (`Collider` or `GroupedCollider`).** A `Collider` without a `RigidBody` is never simulated, and there is no static body type (use a kinematic body).
5. **Collisions are messages.** The physics set drains `RapierPhysicsWorld`'s event list each step, so game code reads `ctx.messages::<CollisionStarted>()`, not `get_collision_events()`. Overlap queries (`are_intersecting`, `get_current_intersections`) read Rapier's narrow phase and still work.
6. **GroupedCollider**: for composite tiles, use `GroupedCollider` from `graphics/tile.rs`.
