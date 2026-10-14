# AGENTS.md - Physics Module

Rapier2D behind `Physics` (a resource), driven by the systems in `src/core/physics/sync.rs`. Usage is covered by the `ryo-physics` skill.

## Coordinate System

Rapier uses meters, Ryo2D uses pixels:

- **100 pixels = 1 meter** by default (`PhysicsConfig::pixels_per_meter`, Rapier's `length_unit`)
- Conversion happens automatically in `Physics`
- Gravity is specified in pixels/s² via the `PhysicsConfig` resource (default `(0.0, 980.0)`, +Y = down) with `pixels_per_meter` (default 100) and `solver_iterations` (default 4)

## Geometry

- A `Collider` is placed by its entity's world transform (`GlobalTransform`) plus `Collider.offset` and `Collider.rotation`, and its shape follows the entity's world scale, mirroring and skew included. `Collider::world_bounds(global)` gives that geometry; particles, navigation and the collider overlay use it, so everything agrees with physics. A zero scale takes a collider out until it grows.
- A visual scale effect (squash and stretch, a pulse) on an entity with a collider scales the hitbox too: put the sprite on a child entity and scale that instead.
- A sprite drawn over a collider uses the default `Anchor::Center`; a top-left-anchored sprite needs the collider offset by half its size.

## The Step

`FixedSet::Physics`, every fixed step while `PhysicsPaused` is false (the scene manager sets it from the top scene's `is_physics_paused` and the stack shape), chained:

1. `forget_removed`: joints, colliders and bodies whose components went away (despawn included; a removed body's joints go inert); entities that lost their `RigidBody` or their parent are queued so their colliders find a new owner.
2. `track_disabled`: disabled entities (bodies, colliders, joints) leave the simulation, re-enabled ones come back with the edits made meanwhile (a re-enabled joint is queued for `sync_joints`, so it is made after its bodies are registered and moved).
3. `propagate_transforms`: world poses are current before physics reads them.
4. `register_bodies`: new bodies at their world pose; an edited `RigidBody` is applied in place.
5. `sync_colliders`: colliders that changed, moved, scaled or changed owner (registered, reparented, reshaped, re-placed).
6. `push_changes`: a body whose world pose changed is moved (a kinematic body travels there over the step); a changed `Velocity` sets its velocity.
7. `sync_joints`: new and re-enabled joints are made; edits apply in place (a break force is only recorded; motor, limits, collisions and the kind's own values, such as a winch's rope length, replace Rapier's joint data from the poses it was made at) or remake it (bodies, kind, anchors); inert ones (a body missing) are retried, and an edit to an inert joint is only recorded. After `push_changes`, so a joint made as the game moves its body anchors where the body was moved.
8. `step_simulation`: `dt` is `time.fixed.timestep()`, so physics follows game speed, pause and hitstop. Afterwards joints pulled past their break force are removed from Rapier.
9. `read_back`: simulated bodies write their pose into their local `Transform` (through their parent) and their velocities.
10. `emit_messages`: the step's contacts and forces become `CollisionStarted` / `CollisionStopped` (with collider and body entities, the trigger flag and the first contact point and normal, captured in the event handler while Rapier still holds the pair) and `ContactForce`; broken joints send `JointBroken` and lose their `Joint` component.

## Notes for AI Agents

1. **Sync is change-detected and compared by value.** Physics' write-back is an ordinary change (propagation must see bodies move), so `push_changes` cannot tell it from a game write by change detection alone: it pushes a pose or `Velocity` only when it differs from the body's. A body nobody touched stays asleep.
2. **Removal bookkeeping also runs in `Last`** every frame, before the trackers are cleared: `forget_removed` and `track_disabled` only. Removal events survive two tracker clears, and at render rates above the fixed rate no fixed step may run in between; without it a despawned body would stay in Rapier. Added and edited bodies are tick-based and wait for the next fixed step.
3. **Bodies and colliders may sit anywhere in the hierarchy.** A collider belongs to the nearest `RigidBody` at or above its entity. Physics reads world poses from `GlobalTransform` and writes a body back into its local `Transform` through its parent. A body's rotation is its local direction through its parent's linear map; its own scale (mirroring included) stays with its shape.
4. **A `Collider` without a `RigidBody` above it is a fixed collider.** `RigidBody::{dynamic, fixed, kinematic}` are the body types.
5. **Joint break forces read every impulse Rapier keeps.** A joint's linear impulse sits in `ImpulseJoint::impulses` for locked axes (revolute, fixed, a prismatic's cross axis), in `data.limits[axis].impulse` for limits (rope, a prismatic's range) and in `data.motors[axis].impulse` for motors (spring, a prismatic's motor). Each is one solver substep's impulse (Rapier keeps only the last substep's), so the force is the sum's length divided by `dt / num_solver_iterations`. That measures a sustained pull, a load held through a step; a sudden jerk is amplified or missed depending on the substep it lands in, so it may break a joint early or not at all. The rustdoc says so. A hinge's frame on b is turned by the bodies' relative angle at registration, so its limits and position target count from there (Box2D's referenceAngle).
6. **Collisions are messages.** The physics set drains `Physics`'s event list each step, so game code reads `ctx.messages::<CollisionStarted>()`, not `get_collision_events()`. Contact state (`contacts`, `overlaps`, `in_contact`) reads Rapier's narrow phase, so it sees solid contacts and trigger overlaps as the last step left them.
