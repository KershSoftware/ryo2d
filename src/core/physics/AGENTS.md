# AGENTS.md - Physics Module

Rapier2D 0.36 behind `Physics` (a resource), driven by the systems in `src/core/physics/sync.rs`. Usage is covered by the `ryo-physics` skill; the design and its decisions are in `docs/specs/2026-10-13-physics-simulation-design.md`.

## Layout

| File | What |
|------|------|
| `body.rs`, `collider.rs`, `joint.rs` | The components: `RigidBody`, `AngularVelocity`, `Collider`, `ColliderShape`, `Joint` |
| `events.rs` | The messages: `CollisionStarted`, `CollisionStopped`, `ContactForce`, `JointBroken`, and `Contact` |
| `query.rs` | `QueryFilter`, `Hit`, `Overlap`, `PointHit`, `Touch`, and the `Physics` query, force and contact-state methods |
| `world.rs` | `Physics`: the Rapier sets, the entity maps, the event collector, queued pushes |
| `sync.rs` | The systems that keep the world and Rapier in step |
| `shape.rs` | `ColliderShape` to a Rapier shape under a collider's linear map |
| `convert.rs` | The glam 0.31 / 0.33 boundary, the sync tolerances |
| `debug.rs` | The F2 overlay |

`rapier2d` is named only in this module. The engine's glam (0.31) and Rapier's (0.33) are different crates: every vector crossing the boundary goes through `convert.rs`.

## Units

The game works in pixels. `PhysicsConfig::pixels_per_meter` (default 100) is Rapier's `length_unit`: tolerances, sleep thresholds and the speed cap are tuned for objects about a metre across. Gravity is in px/s² (default `(0, 980)`, +Y down), a collider's `density` is mass per square metre, forces are mass·px/s² and impulses mass·px/s. `solver_iterations` (default 4) sets the solver effort. `Physics` converts at its edges; nothing else does.

## The Step

`FixedSet::Physics`, every fixed step while `PhysicsPaused` is false (the scene manager sets it from the top scene's `is_physics_paused` and the stack shape), chained as `register_physics_systems` lists them:

1. `forget_removed`: joints, colliders and bodies whose components went away (despawn included; a removed body's joints go inert); entities that lost their `RigidBody` or their parent are queued so their colliders find a new owner.
2. `track_disabled`: disabled entities (bodies, colliders, joints) leave the simulation; re-enabled ones come back with the edits made meanwhile (`Transform`, `Velocity`, `RigidBody`, `Collider`, `Joint`). A re-enabled joint is queued for `sync_joints`, so it is made after its bodies are registered and moved.
3. `propagate_transforms`: world poses are current before physics reads them.
4. `register_bodies`: new bodies at their world pose; an edited `RigidBody` is applied in place. A body refused for a non-finite pose (logged once) is retried every step until its pose is finite; its colliders wait for it. A dynamic body made kinematic or fixed drops the velocity physics wrote back; a velocity the game set in the same tick is kept.
5. `sync_colliders`: colliders that changed, moved, scaled or changed owner (registered, reparented, reshaped, re-placed).
6. `push_changes`: a body whose world pose changed is moved (a kinematic body travels there over the step); a changed `Velocity` or `AngularVelocity` sets its velocity; velocity-driven kinematic bodies are driven; last, the queued forces and impulses are applied.
7. `sync_joints`: new and re-enabled joints are made; edits apply in place or remake the joint (see Joints). After `push_changes`, so a joint made as the game moves its body anchors where the body was moved.
8. `step_simulation`: Rapier steps by `time.fixed.timestep()`, so physics follows game speed, pause and hitstop. Afterwards joints pulled past their break force are removed from Rapier.
9. `read_back`: simulated bodies write their pose into their local `Transform` (through their parent) and their velocities. Every write is worked out before any lands, each parent placed where the write-back will leave it (its continuous rotation, not Rapier's wrapped angle, which rounds differently at large angles).
10. `emit_messages`: the step's contacts and forces become `CollisionStarted` / `CollisionStopped` and `ContactForce`; broken joints send `JointBroken` and lose their `Joint` component.

The order is the dependency order: removals and disables first so nothing below touches a stale handle, propagation before anything reads a world pose, registration before colliders (they need an owner) and colliders before pushes, joints after pushes, and messages last so they describe the step just taken.

`Last` also runs `forget_removed` and `track_disabled` every frame, before `clear_trackers`. Removal events survive two tracker clears, and at render rates above the fixed rate no fixed step may run in between; without it a despawned body would stay in Rapier. In `Last` an entity that lost its body or parent is recorded for relinking too. Added and edited bodies are tick-based and wait for the next fixed step.

`max_delta` (250 ms by default) caps the game time one frame adds, after speed scaling, so a stall cannot run an unbounded number of steps: a 250 ms stall at 60 Hz runs 14 fixed steps (the timestep rounds to whole nanoseconds), at most 15.

## Sync Rules

- **Change-detected and compared by value.** Physics' write-back is an ordinary change (propagation must see bodies move), so `push_changes` cannot tell it from a game write by change detection alone: it pushes a pose or velocity only when it differs from the body's, beyond `1e-3` px and `1e-4` rad (`convert.rs`). A body nobody touched stays asleep.
- **Read back** only awake dynamic bodies and velocity-driven kinematic bodies' poses; a body writes zero velocities on the step it falls asleep. Rotation is written back continuously (no jump at the turn), unless the body locks its rotation.
- **Kinematic bodies** are moved by writing `Transform` (the body travels there over the step, carrying what rides it) or by `Velocity` / `AngularVelocity` (input only, never read back). When both are written in one step the `Transform` wins, but a `Transform` write that leaves the pose unchanged does not count. A parent's move counts too: a velocity-driven kinematic body under a parent the game moves does not integrate its velocity that step. A game that keeps its own movement velocity must not store it in the engine's `Velocity` on a kinematic body, or physics will keep integrating it (bullet-hell keeps it in a game component).
- **Invalid input** (a non-finite pose, a degenerate shape, non-finite joint values) is logged once per entity for as long as it keeps a body, collider or joint: leaving the simulation and coming back (a pickup scaled from zero each time it respawns) does not log again. `forget_removed` forgets an entity's warnings when its last physics component goes, registered or not.
- **Forces and impulses** are queued on `Physics` and applied at the end of `push_changes`, just before the step. While physics is paused they accumulate and apply together at the next step. Forces last one step; impulses are instant. They are ignored for bodies that are not dynamic.

## Hierarchy

- A collider belongs to the nearest `RigidBody` at or above its entity; one with no body above it is a fixed collider of its own. Bodies may sit anywhere in the hierarchy.
- A collider's *placement* is its pose relative to the body plus the linear map of its shape (the world scale, mirroring and rotation), so colliders follow world scale. `Collider::world_bounds(global)` gives that geometry; particles, navigation and the overlay use it, so everything agrees with physics. A zero scale takes a collider out of the simulation until it grows.
- Physics reads world poses from `GlobalTransform` and writes a body into its local `Transform` through its parent. A body's rotation is its local direction through its parent's linear map; its own scale (mirroring included) stays with its shape.
- Under a rotated parent with non-uniform scale a child's shape is sheared; shear is approximated by the convex hull of the mapped shape.
- Squash and stretch (a scale effect) on an entity with a collider scales the hitbox too: put the sprite on a child and scale that.

## Shapes

`shape::build(shape, map)` makes the Rapier shape. A rect stays a cuboid under a diagonal map (scale and mirroring only); a circle stays a ball under a uniform scale, a capsule a capsule. Under any other map (skew from rotated non-uniform parents) a rect becomes the hull of its mapped corners and round shapes become 24-point polygons. Polygons are always hulls (concave shapes are several colliders). Degenerate shapes (a zero, negative or non-finite size, fewer than three polygon points, a zero-length segment, a polyline without an edge, a flattening map) and zero scale leave the simulation, logged once per entity. parry panics on some degenerate hulls, so `shape.rs` refuses them before Rapier sees them.

## Events and Contact State

- An `EventCollector` handler captures the first contact point and normal during the step, while Rapier still holds the pair; `emit_messages` reads the collected list afterwards.
- `Physics::departed` keeps removed colliders' owners until their `Stopped` is reported, so `CollisionStopped` still names entities that were just despawned (either may be gone from the world).
- `contacts`, `overlaps` and `in_contact` read Rapier's narrow phase, so they see solid contacts and trigger overlaps as the last step left them. They report per collider: a collider touching two of a body's colliders is listed for each.
- Queries (`raycast`, `shape_cast`, `overlap_*`, `nearest`) also see the world as the last step left it; a body spawned or moved this tick is not in them until the next step. `QueryFilter` skips triggers by default. `nearest` reports the closest surface point and whether the query point is inside; a ray starting inside a collider hits at distance 0 with a zero normal.

## Joints

- A `Joint` component on any entity names two body entities. Entries are made by `sync_joints` once both bodies are simulated; until then the joint is inert (logged once) and retried each step. Despawning a body leaves its joints inert; disabling the joint's entity removes it until it is re-enabled.
- Anchors are resolved at registration: a world point (`at`) becomes each body's local anchor then. A hinge's frame on b is turned by the bodies' relative angle at registration, so revolute limits and position-motor targets count from there (Box2D's referenceAngle); a slider's position is the distance between its anchors along the axis. A fixed joint ignores `with_anchors` and keeps the relative pose it was made at.
- Edits apply in place (a break force is only recorded; motor, limits, collisions and the kind's own values, such as a spring's rest length, replace Rapier's joint data from the poses it was made at) or remake the joint (bodies, kind, anchors). An unchanged joint never wakes its bodies. An edit to an inert joint is only recorded.
- **Breaking** is decided after the step: a joint whose impulse exceeds `break_force` is removed and reported. Break force reads every impulse Rapier keeps: the linear impulse of locked axes (revolute, fixed, a prismatic's cross axis), `limits[axis].impulse` (rope, a prismatic's range) and `motors[axis].impulse` (spring, a prismatic's motor). Each is one solver substep's impulse (Rapier keeps only the last substep's), so the force is the sum's length divided by `dt / num_solver_iterations`. That measures a sustained pull, a load held through a step; a sudden jerk is amplified or missed depending on the substep it lands in, so it may break a joint early or not at all depending on timing and solver iterations. The rustdoc says so.

## Notes for AI Agents

1. **Never name Rapier types outside this module** and never `unwrap` in library code; vectors cross `convert.rs`.
2. **Do not poll the old API.** There is no raw event list on `Physics`: game code reads `ctx.messages::<CollisionStarted>()` and friends.
3. **A write to a `Transform` is a teleport** for a dynamic or fixed body and a move for a kinematic one; keep write-backs ordinary changes so propagation sees them.
4. **Debug overlay (F2)** draws Rapier's own geometry through `DebugDraw`: dynamic, kinematic, fixed, trigger, joint and contact colours from `config::debug::colors`; sleeping bodies and disabled colliders are dimmed. Rapier's style lengths (contact normals, body axes) are in metres, so `debug.rs` scales them by `length_unit`.
5. **Tests** use `test_util.rs`: a world with the physics systems registered and `run(world, schedules, steps)`. Anything about stepping needs a fixed `dt`.
