# Physics Simulation (3a): Rapier 0.36 Core - Design

## Overview

Sub-project 3a of the professional-grade overhaul (`docs/research/2026-10-11-engine-audit.md`, Part 1 §4) rewrites the physics wrapper on **Rapier 0.36**. It adds **Static, Dynamic and Kinematic bodies** with **rotation**, the **full 2D shape set** with **several colliders per body**, **colliders that follow scale**, **bodies anywhere in the hierarchy**, **layers that reach the simulation**, **collision messages with contact data**, **forces and impulses**, **scene queries**, **joints** and a **debug overlay drawn from the simulation itself**. Sub-project 3b (the gameplay layer: character controller, one-way platforms, tilemap colliders) builds on it.

**Why:** the wrapper (rapier2d 0.22) offers dynamic and position-based kinematic bodies only, so every wall and floor is kinematic; rotation is locked on every body; there are two shapes (box, circle) and one collider per entity; a collider ignores scale and rotation, and a body with a parent is dropped from the simulation. `Collider.layer` and `mask` never reach Rapier, so filtering does nothing. Collision messages carry no contact point or normal, overlap checks cannot see triggers, and a kinematic player never triggers a static sensor (Rapier's default active collision types). There are no forces, no queries and no joints. Units are converted by hand (`PIXELS_PER_METER` in about 15 places) instead of with `length_unit`, and every step allocates two event channels. Separately, the 250 ms frame cap is applied before speed scaling, so a stall at 100x runs 1500 fixed steps in one frame.

**Owner decisions (2026-10-13):**
- **Scope:** action games and physics-driven games served equally, split in two: **3a simulation** (this spec), **3b gameplay** (character controller, one-way platforms, trigger helpers, tilemap colliders, knockback through physics, games moved onto the controller).
- **Approach:** a few rich components that mirror Rapier (`RigidBody`, `Collider`, `Joint`) with live state in separate components (`Velocity`, `AngularVelocity`) and forces, impulses and queries through one `Physics` resource; rich components are `#[non_exhaustive]` and built with builders, so new fields never break games.
- **Step cap:** `max_delta` (250 ms) applies after speed scaling.
- **Breaking changes:** acceptable; networking stays out of scope beyond what compiles.

**Out of scope:** everything in 3b; determinism and snapshots (for networking's later rethink); multibody joints; soft bodies; rounded shapes; per-collider combine rules; a physics-driven particle system (particles keep their CPU collision until their rewrite).

## Architecture

**Module** (`src/core/physics/`):

| File | Holds |
|---|---|
| `body.rs` | `RigidBody`, `BodyKind`, `AngularVelocity` |
| `collider.rs` | `Collider`, `ColliderShape` |
| `joint.rs` | `Joint` |
| `events.rs` | `CollisionStarted`, `CollisionStopped`, `Contact`, `ContactForce`, `JointBroken` (moved out of `messages/builtin.rs`, still registered by the message registry) |
| `query.rs` | `Physics` (the public resource), `QueryFilter`, `Hit`, `Overlap`, `PointHit`, `Touch` |
| `world.rs` | crate-internal Rapier state: sets, pipeline, broad and narrow phase, entity↔handle maps, the event collector |
| `sync.rs` | the systems (moved from `systems/physics.rs`) |
| `debug.rs` | the F2 overlay |

- **Rapier is private.** `rapier2d` is named only inside `src/core/physics/`; no Rapier type appears in a public signature. `RapierPhysicsWorld` is removed.
- **Access.** `SceneContext::physics(&self) -> &Physics` and `SceneContext::physics_mut(&mut self) -> Mut<'_, Physics>`; systems take `Res<Physics>` / `ResMut<Physics>`.
- **Units.** Rapier simulates in pixels. `IntegrationParameters::length_unit` is `PhysicsConfig::pixels_per_meter` (default 100), which scales Rapier's tolerances, sleep thresholds and speed cap. Gravity is `Vec2` in px/s² (default `(0, 980)`, +Y down). Density is per square metre (`density / pixels_per_meter²` per px² inside Rapier), so a 100×100 px box at density 1 weighs 1; masses are in those units and impulses in mass·px/s.
- **Events** are collected by the engine's own Rapier event handler into reusable buffers (no channels), with each started contact's pair read while it is available.

### The step

`FixedSet::Physics`, every fixed step while `PhysicsPaused` is false, chained:

1. **Bookkeeping:** removed bodies, colliders and joints (despawn included), disabled and re-enabled entities. Also runs in `Last` every frame, as today.
2. **Propagate transforms** (the existing change-detected `propagate_transforms`; nothing to do when nothing moved), so physics sees this step's world poses, including entities under a parent the game just moved.
3. **Register** new bodies, colliders and joints; apply edited ones in place.
4. **Push game changes:**
   - a body whose world pose (translation and rotation from its `GlobalTransform`) differs from Rapier's by more than 1e-3 px or 1e-4 rad is moved: dynamic and fixed bodies are teleported, a kinematic body travels there over the step;
   - a standalone collider whose world pose or scale changed is moved or rebuilt;
   - a changed `Velocity` / `AngularVelocity` that differs from Rapier's is pushed (dynamic), or integrated into the next pose (kinematic, when its pose was not written);
   - queued forces and impulses are applied.
5. **Step** with `dt` = the fixed timestep.
6. **Read back** awake dynamic bodies and velocity-driven kinematic bodies: world pose → local `Transform` through the parent's `GlobalTransform` (local scale kept); dynamic bodies also write `Velocity` and `AngularVelocity`; values are written only when they differ.
7. **Emit** messages; check joint break forces.

`FixedSet::Transforms` then propagates what physics moved. The tolerance in step 4 keeps float round trips through a parent from counting as moves, so bodies nobody touched stay asleep.

### Time

`GameClock::advance` caps the scaled delta: `delta = min(real_delta × speed, max(max_delta, one fixed step))`. At 1x nothing changes; a stall or a high speed advances at most 250 ms of game time per frame (15 steps at 60 Hz, 36 at 144 Hz), so above about 15x at 60 fps the game runs as fast as that allows. `PhysicsPaused`, hitstop and slow motion behave as today.

## Bodies

`RigidBody` (`#[non_exhaustive]`, public fields, built with builders; requires `Transform`, `Velocity` and `AngularVelocity`):

| Builder | `BodyKind` | Behaviour |
|---|---|---|
| `RigidBody::dynamic()` | `Dynamic` | simulated: gravity, contacts, forces, joints |
| `RigidBody::fixed()` | `Fixed` | static: never moves by itself; can be teleported through `Transform`; carries nothing |
| `RigidBody::kinematic()` | `Kinematic` | moved only by the game, never pushed |

- **A `Collider` with no `RigidBody` above it is a fixed collider.** `RigidBody::fixed()` is for static structures that group child colliders.
- **Kinematic:** write `Transform` and the body travels there over the step; Rapier derives its velocity, so bodies riding it are carried and bodies it meets are pushed. Or set `Velocity` / `AngularVelocity` and the engine integrates them and writes `Transform` back. When both are written in one step the `Transform` wins. Velocity on a kinematic body is input only and never read back.
- **Settings** (builder → field, default): `with_gravity_scale(f)` → `gravity_scale` 1.0; `with_linear_damping(f)` → `linear_damping` 0 (was `drag`); `with_angular_damping(f)` → `angular_damping` 0; `lock_rotation()` → `lock_rotation` false (**dynamic bodies rotate by default**); `with_ccd()` → `ccd` false; `never_sleep()` → `can_sleep` true; `with_dominance(i8)` → `dominance` 0 (a higher dominance pushes lower ones and is never pushed back). Settings that do not apply to a kind (gravity on a fixed body) are ignored, and the rustdoc says so. Edits apply in place (body type, settings) without resetting contacts or sleep.
- **Mass** comes from the body's colliders (density or explicit mass) and sums over them.
- **Live state:** `Velocity { dx, dy }` (world px/s, unchanged, still replicated by networking) and `AngularVelocity(f32)` (new; rad/s, same sign as `Transform.rotation`).
- **Forces and impulses** on `Physics` (`&mut self`), queued and applied before the next step, so they work on a body spawned the same step: `apply_impulse(e, Vec2)`, `apply_impulse_at(e, impulse, world_point)`, `apply_angular_impulse(e, f32)` (instant); `apply_force(e, Vec2)`, `apply_force_at(e, force, world_point)`, `apply_torque(e, f32)` (for one step: call every step for a steady push). Calls naming a non-dynamic or unknown entity are ignored. Reads: `mass(e) -> Option<f32>`, `is_sleeping(e) -> bool`.
- **Lifecycle:** a `Disabled` entity's body and colliders are disabled in Rapier, not removed, so velocity survives re-enabling; despawn removes the body, its colliders and its joints. Knockback keeps adding to `Velocity` on dynamic bodies (3b revisits it).

## Colliders

`Collider` (`#[non_exhaustive]`, public fields, built with builders; requires `Transform`). `ColliderShape` is the public enum behind the builders, and queries take it too:

| Builder | Shape |
|---|---|
| `Collider::rect(w, h)` | box (was `box_collider`, `ColliderShape::Box`) |
| `Collider::circle(r)` | circle (was `circle_collider`) |
| `Collider::capsule(height, radius)` | vertical capsule; `height` includes the caps; rotate it for horizontal |
| `Collider::polygon(points)` | convex hull of the points |
| `Collider::segment(a, b)` | one edge |
| `Collider::polyline(points)` | open chain (terrain, level outlines) |

- **Invalid shapes** (zero, negative or non-finite sizes, a degenerate polygon, a polyline under two points) are logged once per entity and left out of the simulation; nothing panics. Concave shapes are several colliders.
- **Placement:** `with_offset(x, y)`, `with_rotation(radians)`, relative to the entity.
- **Several colliders per body.** A `Collider` attaches to the nearest `RigidBody` on its own entity or an ancestor; its pose relative to the body is its world pose relative to the body's, plus offset and rotation. Moving a child collider entity moves the collider in place; reparenting moves it to the new body; a nearer `RigidBody` starts a new body. One `Collider` per entity: more shapes go on child entities.
- **Scale.** A collider's shape is built with its entity's world scale (which includes the body's own scale; the body pose itself carries no scale). Rects stay rects; circles and capsules under non-uniform scale become polygon approximations; polygons, segments and polylines scale their points, and negative scale mirrors them. A scale change rebuilds the shape in place. The docs recommend squash-and-stretch on a child sprite so juice does not resize hitboxes.
- **Material:** `with_density(d)` (default 1 per m²) or `with_mass(m)` (this collider's mass, inertia from its shape); `with_friction(f)` (default 0.5); `with_restitution(r)` (default **0**; was 0.5). Combine rules are Box2D's, engine-wide: friction is the geometric mean (0 on either side is frictionless), restitution the maximum.
- **Layers:** `with_layer(bits)` and `with_mask(bits)` (defaults layer 1, mask all). Two colliders interact only if each one's layer is in the other's mask (Rapier's `InteractionGroups`, AND test); this filters contacts, triggers and events.
- **Triggers:** `as_trigger()` makes a sensor: overlaps, no push. Every collider enables all active collision types except fixed–fixed, so triggers and contacts report for every pair of body kinds except two static ones (a kinematic player entering a static trigger is now detected).
- **Force events:** `with_force_events(threshold)` opts the collider into `ContactForce` messages.
- **Edits** to any field apply in place on the next step.
- **Geometry helper:** `Collider::world_bounds(&self, &GlobalTransform) -> (Vec2, Vec2)`, given its entity's `GlobalTransform`, gives the world AABB for any shape (replacing `world_center` and `world_bounds(position)`); navigation and leap-king use it. Particles collide with circles exactly and with every other shape by its world bounds until the particles rewrite. `should_collide_with` is removed.
- **`GroupedCollider` is removed.** `CompositeTileBuilder::build_with_collider()` returns a `Collider` (a rect offset to its centre); `physics/` no longer imports from `graphics/`.

## Events

Emitted after each step and read the next step with `ctx.messages::<T>()` (messages still survive frames with no step).

- **`CollisionStarted` / `CollisionStopped`:** `a`, `b` (collider entities), `body_a`, `body_b` (the owning body entity, or the collider's own entity when it has none), `trigger: bool` (either side is a trigger); `CollisionStarted` also has `contact: Option<Contact>` with `Contact { point: Vec2, normal: Vec2 }` (the first contact; normal from a to b; `None` for triggers). Helpers: `involves(e)` (collider or body), `other(e) -> Option<Entity>` (the other body), `other_collider(e) -> Option<Entity>`. The order of a and b is unspecified; on `CollisionStopped` either entity may already be despawned.
- **`ContactForce`:** `a`, `b`, `body_a`, `body_b`, `force: f32` (total magnitude), `direction: Vec2` (of the largest force), `started: bool` (true on the first step above the threshold); sent each step the pair's total contact force exceeds either collider's threshold.
- **`JointBroken`:** `joint`, `a`, `b` (see Joints).
- **Removed:** `PhysicsCollisionEvent`, `CollisionEventType`, the world's event list (`get_collision_events`, `drain_collision_events`, `get_collision_events_for`, `has_collision`, `collision_event_count`).

## Queries

On `Physics` (`&self`). They see the world as the last step left it (a body spawned or teleported this step is found after the next step).

- `raycast(origin, direction, max_distance, &QueryFilter) -> Option<Hit>`; `raycast_all(..) -> Vec<Hit>` nearest first. A zero or non-finite direction returns `None` / empty.
- `shape_cast(&ColliderShape, origin, rotation, direction, max_distance, &QueryFilter) -> Option<Hit>`: sweeps a shape; a shape that starts overlapping hits at distance 0.
- `overlap_point(point, &QueryFilter) -> Vec<Overlap>`; `overlap_shape(&ColliderShape, position, rotation, &QueryFilter) -> Vec<Overlap>`.
- `nearest(point, max_distance, &QueryFilter) -> Option<PointHit>`.
- **Types:** `Hit { collider, body, point, normal, distance }`, `Overlap { collider, body }`, `PointHit { collider, body, point, inside }`.
- **`QueryFilter`** (`Default` hits everything solid): `with_mask(bits)` (a collider is hit when its layer meets the mask, whatever its own mask); `excluding(e)` (a body with all its colliders, or one collider; repeatable); `with_triggers()` (**triggers are excluded by default**, as in Godot, so a line-of-sight ray is not stopped by a trigger zone).
- **Contact state:** `contacts(e) -> Vec<Touch>` with `Touch { collider, body, point, normal, impulse }`, the solid contacts touching any of e's colliders (e a body or a collider), normal from e toward the other (ground below is `(0, +1)`); `overlaps(e) -> Vec<Overlap>`, current trigger overlaps; `in_contact(a, b) -> bool`, solid or trigger. These replace `are_intersecting` and `get_current_intersections`.

## Joints

`Joint` (`#[non_exhaustive]`, built with builders) on its own entity, or any entity, connecting two body entities `a` and `b`:

| Builder | Joint |
|---|---|
| `Joint::revolute(a, b)` | hinge or wheel; pivot defaults to b's origin |
| `Joint::prismatic(a, b, axis)` | slider along `axis`, in a's frame |
| `Joint::fixed(a, b)` | weld keeping the bodies' relative pose at registration |
| `Joint::rope(a, b, max_length)` | maximum distance, slack below it |
| `Joint::spring(a, b, rest_length, stiffness, damping)` | damped spring |

- **Builders:** `at(world_point)` (the pivot in world space; local anchors computed from the bodies' poses at registration); `with_anchors(local_a, local_b)`; `with_limits(min, max)` (radians for revolute, px for prismatic); `with_motor_velocity(target, max_force)` and `with_motor_position(target, stiffness, damping)` (revolute, prismatic); `with_collisions()` (connected bodies collide; off by default, as Box2D's `collideConnected`); `with_break_force(f)`. Builders that do not apply to a kind are ignored, and the rustdoc says so.
- **Breaking:** after each step the engine reads each breakable joint's impulse; above `break_force × dt` the joint is removed from Rapier, its `Joint` component is removed, and `JointBroken { joint, a, b }` is sent.
- **Lifecycle:** edits apply in place on the next step, compared by value so an unchanged joint never wakes its bodies; despawning or disabling the joint's entity removes or disables it; despawning either body removes it from Rapier and leaves the `Joint` inert. A joint whose `a` or `b` is not a `RigidBody` entity is inert and logged once; nothing panics.
- Only Rapier's impulse joints are used.

## Debug overlay

- **F2** draws Rapier's own geometry through its `debug-render` pipeline into `DebugDraw`: every shape after scale, rotation and offset, joints, and contact points. Colours by kind: dynamic, kinematic, fixed, trigger; sleeping bodies dimmed. It replaces the overlay that drew `Collider` and `GroupedCollider` data.
- **F3** (velocity arrows) is unchanged.

## Configuration

`PhysicsConfig` (`#[non_exhaustive]`, resource, from `EngineConfig::physics`): `gravity: Vec2` (editable live), `pixels_per_meter: f32` (100), `solver_iterations: usize` (4). Builders `with_gravity(Vec2)`, `zero_gravity()`, `with_pixels_per_meter(f32)`, `with_solver_iterations(usize)`.

## Public API changes

**Added:** `Physics`, `SceneContext::physics`, `SceneContext::physics_mut`; `BodyKind`, `AngularVelocity`; `RigidBody::{dynamic, fixed, kinematic}` and the new settings; `Collider::{rect, circle, capsule, polygon, segment, polyline}`, `with_rotation`, `with_density`, `with_mass`, `with_force_events`; `Joint`; `QueryFilter`, `Hit`, `Overlap`, `PointHit`, `Touch`, `Contact`; `ContactForce`, `JointBroken`; `PhysicsConfig::{pixels_per_meter, solver_iterations}`.

**Changed:** `RigidBody` fields (`kind` replaces `is_kinematic`; `linear_damping` replaces `drag`); `RigidBody::new()` becomes `dynamic()`; dynamic bodies rotate by default; a bare `Collider` is a fixed collider; default restitution 0 and friction 0.5 with Box2D combine rules; layers filter the simulation; `CollisionStarted` / `CollisionStopped` fields; `Collider::world_bounds` takes a `GlobalTransform`; `PhysicsConfig::gravity` is a `Vec2`; `CompositeTileBuilder::build_with_collider` returns a `Collider`; `max_delta` applies after speed scaling; bodies may have parents.

**Removed:** `RapierPhysicsWorld`, `PhysicsCollisionEvent`, `CollisionEventType`, `GroupedCollider`, `ColliderShape::Box`, `Collider::{box_collider, circle_collider, world_center, should_collide_with}`, `RigidBody::new`, `RigidBody::{is_kinematic, drag}`, `PIXELS_PER_METER`, the parented-body drop and its warning.

## Migration (in place, every step green)

Each step ports the games, demos and tests its API change breaks.

0. **Baseline:** the `physics` benchmark (below) on the current wrapper, number recorded.
1. **Rapier 0.36** behind today's API: glam types, the BVH broad phase, the new `step`, the engine's own event handler, `length_unit` replacing hand conversion; `rapier2d` builds for wasm32; the `max_delta` change.
2. **Bodies:** `BodyKind` with `Fixed`, bare colliders as fixed colliders, rotation and `AngularVelocity`, the settings, velocity-driven kinematic bodies, `Disabled` via Rapier's enabled flag, `Physics` with forces and impulses (walls and floors become fixed).
3. **Colliders:** the shape set, placement, material defaults and combine rules, layers into Rapier, all active collision types, edits in place, invalid shapes, `world_bounds`; `GroupedCollider` removed.
4. **Hierarchy and scale:** propagation before the step, colliders on descendants, bodies with parents and world→local write-back, scaled shapes.
5. **Events and queries:** contact data, `ContactForce`, every query, contact state.
6. **Joints** and breaking.
7. **The overlay** from Rapier's debug render.
8. **The `physics` demo**, documentation and verification.

## Games and demos

Ported to the new API, keeping their own movement code (3b moves them onto the controller):

- **breakout:** walls, blocks and the death zone fixed; the paddle kinematic; message fields renamed.
- **flappy:** `GroupedCollider` → `Collider`; the bird's rotation locked.
- **bullet-hell:** platforms and barriers fixed; player, turrets and drones kinematic with hand-written movement; bullets dynamic triggers.
- **leap-king, networking demo:** their bare colliders become fixed colliders in Rapier; `world_bounds` takes the `GlobalTransform`.
- **time-control, knockback demos:** walls fixed, rotation locked where the page needs it.
- **New `physics` demo** (demo-kit standard, no files, runs on the web): *Shapes* (a pile of boxes, circles, capsules and polygons; click to spawn), *Joints* (pendulum, chain bridge with a break force, motorised car, spring), *Queries* (a raycast from the mouse with its normal, a shape cast, a highlighted overlap region), *Layers* (layers passing through each other, a trigger counting overlaps, impact flashes from `ContactForce`).

## Testing

- **Unit (physics module):** a body falls `½·g·t²` px in pixel units at any fixed rate; a 100×100 px box at density 1 has mass 1; a fixed body never moves and a bare collider is fixed; a kinematic platform moved by `Transform` carries a resting box; a kinematic body moved by `Velocity` writes `Transform` back; a dynamic box landing on a corner rotates and `AngularVelocity` reads back; `lock_rotation` holds; dominance lets the higher body push without being pushed; a child collider adds mass and moves with its body; a body under a moving parent reads back correctly into local space; a 2× scaled entity's collider is twice the size; layers filter contacts both ways; a kinematic body entering a fixed trigger produces `CollisionStarted` with `trigger`; `CollisionStarted` carries a normal from a to b; `ContactForce` fires above its threshold only; each query (ray, ray-all order, shape cast from inside, point and shape overlap, nearest, filters: mask, excluding, triggers excluded by default); `contacts` reports ground below as `(0, +1)`; `overlaps` sees triggers; each joint kind holds its constraint; a motor turns; limits hold; a joint past its break force is removed with `JointBroken`; an edited `Collider`, `RigidBody` or `Joint` applies in place and an untouched body stays asleep; a disabled body keeps its velocity; invalid shapes are logged and skipped; forces queued before a body registers apply.
- **Time:** at 100x a 250 ms stall runs at most 15 steps at 60 Hz; 1x is unchanged.
- **E2E (`HeadlessEngine`):** the F2 overlay draws a rotated, scaled rect at its simulated place; collision messages reach a scene; the `physics` demo joins the tour.
- **Gates:** `rapier2d` builds for wasm32 (step 1); the demo tour with every changed page checked by eye and listed; the five games captured and checked by eye; the `physics` demo on the web in Chromium, WebKit and Firefox; the `physics` benchmark (1000 mixed bodies settling, time per step, added to `benches/` beside `foundation`) recorded against the baseline.

## Documentation

- Skills: `ryo-physics` rewritten; `ryo-api` (every signature above); `ryo-game-dev` (recipes: a stack of crates, a rope bridge, a line-of-sight check); copies in `src/cli/skills/`.
- AGENTS.md: `physics/` rewritten (the step, sync rules, hierarchy, units, invariants); `src/core/AGENTS.md` (time: `max_delta` after scaling); `docs/architecture/overview.md` (physics).

## Design decisions

- **Rich components over many small ones:** an agent learns six types whose builders list every option; what changes every frame (`Velocity`, `AngularVelocity`, forces) is already separate, so finer change detection would buy little. `#[non_exhaustive]` keeps additions non-breaking.
- **Pixels through `length_unit`:** one place sets the scale, and Rapier's tolerances and sleep thresholds are tuned for it instead of for 1 m objects.
- **Density per square metre:** masses and impulses stay human-sized.
- **A bare collider is static:** Unity's and Rapier's rule; games need no body for walls.
- **Rotation by default:** what Unity, Godot and Box2D do; locking is one builder call.
- **Box2D combine rules:** a frictionless or bouncy surface behaves as such against any default surface.
- **Triggers out of queries by default:** rays mostly ask "what solid thing is there".
- **Joints on their own entities:** any number of joints per body pair; despawning a joint entity removes exactly that joint.
- **Breaking by impulse, checked after the step:** Rapier has none; this is one comparison per breakable joint.
- **`max_delta` after scaling:** a frame's work is bounded whatever the speed, with no new setting.

## Risks

- Rapier 0.36 facts the research could not confirm (constant names, the query dispatcher, `ActiveCollisionTypes` defaults, wasm32 status) are checked in step 1; a wasm32 failure stops the plan for an owner decision.
- Write-back under a parent with non-uniform scale and rotation cannot represent shear; the body's pose is kept and its local rotation extracted. Documented.
- Rotation by default and the new material defaults change how ported games feel; each is checked by eye.
- Kinematic–fixed events add messages; games that read every `CollisionStarted` filter by entity (breakout already does).
- Queries see the last step's world; 3b's controller needs fresher poses and syncs before querying.
