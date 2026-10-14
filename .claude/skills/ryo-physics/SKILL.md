---
name: ryo-physics
description: Rapier2D physics integration - bodies, colliders, joints, queries, contact state and collision messages.
metadata:
  version: 3.0.0
---

# Ryo2D Physics Specialist

You help users add Rapier2D physics to their games: rigid bodies, colliders, joints, spatial queries and collision messages.

**For complete API signatures, see the `ryo-api` skill.**

## How it runs

An entity enters the simulation when it has a `Collider`, with or without a `RigidBody` (a collider with no body above it is a fixed collider). The engine does the rest in `FixedSet::Physics`, once per fixed step, after the scene's `update` and the scene's fixed systems:

1. Forget what went away (despawned or removed bodies, colliders, joints) and what was disabled.
2. Register new bodies, bring colliders and the game's `Transform` / `Velocity` edits in line, make new joints.
3. Step by the fixed timestep (`EngineBuilder::fixed_hz`, default 60 Hz). Slow motion runs fewer steps; pause and hitstop run none.
4. Write simulated bodies back into `Transform`, `Velocity` and `AngularVelocity` (ordinary component changes, so propagation and rendering see them move).
5. Send `CollisionStarted` / `CollisionStopped` / `ContactForce` / `JointBroken` messages.

Physics pauses while the top scene's `is_physics_paused()` returns `true`, and while a pushed scene on top of another does not return `true` from `updates_below()`. Forces and impulses queued meanwhile accumulate and apply together at the next step.

One frame adds at most `max_delta` (250 ms) of game time after speed scaling, so a 250 ms stall at 60 Hz runs 14 fixed steps (at most 15), never a pile-up.

Units are pixels (100 pixels = 1 Rapier metre, converted internally). +Y is down.

## Which body kind

`RigidBody::dynamic()` (the default), `RigidBody::fixed()`, `RigidBody::kinematic()`:

| Kind | Use it for | How it moves |
|---|---|---|
| Fixed | Walls, floors, level geometry | Never; writing its `Transform` teleports it but carries nothing. A bare `Collider` is fixed too |
| Kinematic | Moving platforms, doors, characters with their own movement | Only by the game, never pushed by contacts |
| Dynamic | Crates, balls, debris, anything physics should push | Gravity, contacts, forces, impulses and joints |

```rust
// Dynamic prop
ctx.world.spawn((
  Transform::from_xy(200.0, 100.0),
  RigidBody::dynamic(),
  Collider::rect(32.0, 32.0).with_restitution(0.2),
));

// Fixed floor: no body needed
ctx.world.spawn((
  Transform::from_xy(400.0, 575.0),
  Collider::rect(800.0, 50.0),
));

// Platform that travels to its Transform over the step, carrying what rides it
let platform = ctx.world.spawn((
  Transform::from_xy(200.0, 400.0),
  RigidBody::kinematic(),
  Collider::rect(100.0, 20.0),
)).id();

// Character: dynamic, never tips over
ctx.world.spawn((
  Transform::from_xy(100.0, 100.0),
  RigidBody::dynamic().lock_rotation().with_ccd(),
  Collider::capsule(40.0, 10.0).with_friction(0.0),
));
```

`RigidBody` fields and builders: `gravity_scale` (`with_gravity_scale`), `linear_damping`, `angular_damping`, `lock_rotation()`, `ccd` (`with_ccd`), `can_sleep` (`never_sleep()`), `dominance` (`with_dominance`: a higher one pushes lower ones and is never pushed back). Settings that do not apply to a kind are ignored. Mass comes from the colliders. Edit a `RigidBody` freely: it applies in place on the next step, keeping contacts and sleep. A `RigidBody` brings `Transform`, `Velocity` and `AngularVelocity` with it.

Moving bodies:

- **Dynamic**: set `Velocity` / `AngularVelocity` (the body's own for a jump or a launch), or use forces and impulses. Writing `Transform` teleports it. Physics writes pose and velocities back every step the body is awake; asleep bodies write zero velocities on the step they fall asleep.
- **Kinematic**: write `Transform` (it travels there over the step) or `Velocity` / `AngularVelocity` (input only, never read back). When both are written in one step the `Transform` wins, but a `Transform` write that leaves the pose unchanged does not count. So a game that keeps its own movement velocity must not store it in the engine's `Velocity` on a kinematic body; keep it in a game component and write `Transform` from it.
- Making a dynamic body kinematic or fixed drops the velocity physics wrote back; a velocity the game set in the same tick is kept.

## Colliders and shapes

```rust
Collider::rect(32.0, 64.0)                       // width, height
Collider::circle(16.0)                           // radius
Collider::capsule(40.0, 10.0)                    // total height (ends included), radius; upright
Collider::polygon([Vec2::new(0.0, -20.0), Vec2::new(20.0, 20.0), Vec2::new(-20.0, 20.0)]) // convex hull
Collider::segment(Vec2::ZERO, Vec2::new(100.0, 0.0))
Collider::polyline([a, b, c, d])                 // open chain: terrain, outlines
Collider::circle(32.0).as_trigger()              // detects overlaps, never pushes
Collider::rect(20.0, 8.0).with_offset(12.0, 0.0) // hitbox to the right of the origin
Collider::rect(40.0, 8.0).with_rotation(0.3)     // turned relative to the entity
```

- A collider is centred on its entity's origin plus `offset`, turned by `rotation`. With the default `Anchor::Center` that is the sprite's centre, so a 32x32 sprite takes `Collider::rect(32.0, 32.0)` with no offset; a top-left-anchored sprite needs the offset of half its size.
- `ColliderShape` is the data (`Rect { size }`, `Circle { radius }`, `Capsule { height, radius }`, `Polygon { points }`, `Segment { a, b }`, `Polyline { points }`); `Collider::shape` holds it and queries take it.
- Concave shapes are several colliders. **Compound shapes**: give a body child entities that each carry a `Collider`; every collider attaches to the nearest body at or above it, and the body's mass is their sum. (Walls of a room: a fixed body with a collider child per wall.)
- A degenerate shape (zero or negative size, a polygon with fewer than three points, a zero-length segment) or a zero scale takes the collider out of the simulation (logged once); it comes back when fixed or when the scale grows.
- `Collider::world_bounds(&GlobalTransform) -> (min, max)` gives the box where physics simulates it.

### Composite tiles

`CompositeTileBuilder::build_with_collider()` returns the tile and a rect `Collider` already offset to cover all the parts:

```rust
let (composite, collider) = CompositeTileBuilder::new()
  .cap(cap_sprite, 0.0, 0.0)
  .body_tiled(body_sprite, 0.0, 32.0, TileDirection::Down, 128.0)
  .build_with_collider();

ctx.world.spawn((Transform::from_xy(100.0, 50.0), RigidBody::fixed(), composite, collider));
```

## Materials

`with_friction` (default 0.5; 0 is ice), `with_restitution` (default 0; 1 bounces back fully), `with_density` (mass per square metre, default 1), `with_mass` (overrides density for this collider). Two surfaces combine as: friction the geometric mean, restitution the larger. A 100x100 px box at density 1 weighs 1; read a body's mass with `physics.mass(entity)`.

## Layers and masks

`layer` is the bits a collider is on (default `1`), `mask` the bits it interacts with (default all). **Two colliders interact only if each one's layer is in the other's mask** (the AND rule). A one-sided mask is not enough.

| Collider | layer | mask | Meant to meet |
|---|---|---|---|
| player | `0b0001` | `0b0110` | enemies and walls |
| enemy | `0b0010` | `0b0101` | player and walls |
| wall | `0b0100` | `0b0011` | player and enemies |
| player bullet | `0b1000` | `0b0010` | enemies only |

Check each pair both ways:

- Player and enemy meet: the enemy's layer `0b0010` is in the player's mask `0b0110`, and the player's layer `0b0001` is in the enemy's mask `0b0101`.
- Player bullet and enemy do not meet as listed: the enemy's layer `0b0010` is in the bullet's mask, but the bullet's layer `0b1000` is not in the enemy's mask `0b0101`. Widen the enemy's mask to `0b1101` and they do.
- Player bullet and wall never meet as listed: the wall's mask `0b0011` lacks `0b1000`, and the bullet's mask lacks the wall's `0b0100`. Both sides must agree.

Use constants:

```rust
const PLAYER: u32 = 1 << 0;
const ENEMY: u32 = 1 << 1;
const WALL: u32 = 1 << 2;
Collider::circle(8.0).with_layer(PLAYER).with_mask(ENEMY | WALL)
```

Queries use `QueryFilter::with_mask`, which is matched against the collider's layer only.

## Triggers

`as_trigger()` colliders detect but never push. They send `CollisionStarted` / `CollisionStopped` with `trigger: true` and `contact: None`, and show in `physics.overlaps(zone)`. Pickups, zones and sensors are triggers on a fixed body (or no body).

## Messages

Read with `ctx.messages::<T>()` in a scene (`MessageReader<T>` in systems); a contact from step N reaches step N+1's `update`.

- `CollisionStarted { a, b, body_a, body_b, trigger, contact: Option<Contact> }` and `CollisionStopped { a, b, body_a, body_b, trigger }`. `a`/`b` are the colliders, `body_a`/`body_b` their bodies (a collider with no body is its own). `contact` is `Contact { point, normal }`, the first contact, the normal pointing from `a` to `b`; `None` for triggers. The order of `a` and `b` is unspecified: use `involves(entity)` (collider or body), `other(entity) -> Option<Entity>` (the other side's body) and `other_collider(entity)`.
- `ContactForce { a, b, body_a, body_b, force, direction, started }`: sent every step a contact pushes above its collider's `with_force_events(threshold)`. Breakables, impact sounds.
- `JointBroken { joint, a, b }`: a joint exceeded its break force and was removed (its `Joint` component too).

```rust
for hit in ctx.messages::<CollisionStarted>() {
  if let Some(other) = hit.other(self.player)
    && let Some(hazard) = ctx.world.get::<Hazard>(other)
  {
    self.health -= hazard.damage;
  }
}
for force in ctx.messages::<ContactForce>() {
  if force.force > 500.0 { /* crack the vase */ }
}
```

On `CollisionStopped` either entity may already be despawned: check before you query it.

## Forces and impulses

Methods on `Physics` (`ctx.physics_mut()`), for dynamic bodies only (others ignore them):

```rust
ctx.physics_mut().apply_impulse(body, Vec2::new(0.0, -300.0));        // instant, mass * px/s
ctx.physics_mut().apply_impulse_at(body, impulse, world_point);        // off-centre also spins
ctx.physics_mut().apply_angular_impulse(body, 5.0);
ctx.physics_mut().apply_force(body, Vec2::new(500.0, 0.0));            // mass * px/s^2, next step only
ctx.physics_mut().apply_force_at(body, force, world_point);
ctx.physics_mut().apply_torque(body, 20.0);
```

Forces last one step: call them every step for a steady push (a thruster in `update`). Impulses are instant. Both queue until the next step, accumulate while physics is paused and apply together when it resumes.

## Queries

All on `&Physics` (`ctx.physics()`), in pixels, and they see the world as the last step left it (a body spawned this tick is not in them yet).

```rust
let filter = QueryFilter::new().excluding(looker); // skips a body (all its colliders) or one collider
let filter = QueryFilter::new().with_mask(WALL | ENEMY).with_triggers();

physics.raycast(origin, direction, max_distance, &filter) -> Option<Hit>
physics.raycast_all(origin, direction, max_distance, &filter) -> Vec<Hit>           // nearest first
physics.shape_cast(&shape, origin, rotation, direction, max_distance, &filter) -> Option<Hit>
physics.overlap_point(point, &filter) -> Vec<Overlap>
physics.overlap_shape(&shape, position, rotation, &filter) -> Vec<Overlap>
physics.nearest(point, max_distance, &filter) -> Option<PointHit>
```

- `QueryFilter` defaults to every layer and **skips triggers** (a trigger zone does not block a line of sight); `with_triggers()` includes them; `with_mask(bits)` matches colliders whose layer meets the bits; `excluding(entity)` can be called repeatedly.
- `Hit { collider, body, point, normal, distance }`. A ray starting inside a collider hits it at distance 0 with a zero normal; a shape that starts overlapping hits at distance 0.
- `Overlap { collider, body }`. `PointHit { collider, body, point, inside }`: `nearest` reports the closest surface point, and `inside` is true when the query point is inside the collider.
- The direction need not be normalised; a zero or non-finite one, or a negative range, returns nothing.

## Contact state

For "am I on the ground", "is anything in this zone":

```rust
let physics = ctx.physics();
// Solid contacts after the last step; the entity is a body (all its colliders) or one collider
let on_ground = physics
  .contacts(player)
  .iter()
  .any(|touch| touch.normal.y > 0.7);          // normal points from the entity toward the other side: ground below is (0, +1)
let inside = physics.overlaps(zone);            // Vec<Overlap>: colliders overlapping a trigger, or triggers the entity is inside
let touching = physics.in_contact(player, platform); // any collider of `a` touches or overlaps any of `b`
let asleep = physics.is_sleeping(crate_entity);
```

`Touch { collider, body, point, normal, impulse }` (the deepest point, and the step's normal impulse).

## Joints

A `Joint` is its own component on any entity (usually a fresh one); it names two body entities `a` and `b`. Both must be simulated; until then the joint waits (logged once). Despawning a body leaves its joints inert; despawning the joint entity removes the joint.

```rust
// Hinge at a world point (b's origin by default)
ctx.world.spawn(Joint::revolute(post, door).at(Vec2::new(300.0, 200.0)).with_limits(-1.2, 1.2));

// Motor-driven wheel
ctx.world.spawn(Joint::revolute(car, wheel).with_motor_velocity(8.0, 5000.0));

// Slider along an axis (in a's frame)
ctx.world.spawn(Joint::prismatic(rail, cart, Vec2::X).with_limits(-100.0, 100.0));

// Weld
ctx.world.spawn(Joint::fixed(hull, turret));

// Rope: at most 120 px between the anchors, slack below
ctx.world.spawn(Joint::rope(anchor, weight, 120.0));

// Spring: rest length, stiffness, damping
ctx.world.spawn(Joint::spring(a, b, 80.0, 40.0, 2.0));
```

Builders: `at(point)`, `with_anchors(a_local, b_local)`, `with_limits(min, max)`, `with_motor_velocity(target, max_force)`, `with_motor_position(target, stiffness, damping)`, `with_collisions()` (the two bodies collide with each other; off by default), `with_break_force(force)`.

- Anchors (`JointAnchor`): `Origins` (default), `World(point)` (from `at`), `Local { a, b }` (from `with_anchors`). Revolute and prismatic pivot at b's origin unless placed; a **fixed joint ignores `with_anchors`** and keeps the relative pose it was made at. Anchors are resolved when the joint is made.
- **Revolute limits and position-motor targets are relative to the bodies' relative angle when the joint was made**: a hinge made at any angle starts at 0, inside its limits. A prismatic's position is the distance between its anchors along the axis.
- Motors: `JointMotor::Velocity { target, max_force }`, `JointMotor::Position { target, stiffness, damping }`. Limits and motors only drive revolute and prismatic joints.
- **Break force measures a sustained pull**, a load held over a step. A sudden jerk may break a joint early or not at all depending on timing and the solver iterations. Breaking removes the joint, sends `JointBroken` and drops its `Joint` component.
- Edit a `Joint` in place (limits, motor, collisions, break force, lengths); changing the bodies, kind or anchors remakes it. Non-finite values are ignored (logged once).

## Units and `PhysicsConfig`

A resource: `ctx.resource_mut::<PhysicsConfig>()` edits it live, effective next step. `gravity` (default `(0, 980)` px/s², +Y down), `pixels_per_meter` (default 100: Rapier's tolerances and sleep thresholds are tuned for objects about a metre across, so keep pixel sizes in the range of 20 to 300 px per body or change the scale), `solver_iterations` (default 4; more is stiffer stacks and joints, at a cost). Builders: `with_gravity`, `zero_gravity()` (top-down and space games), `with_pixels_per_meter`, `with_solver_iterations`. Insert it before `run()` with `EngineBuilder::insert_resource`.

Density is per square metre: a 32x32 px box at density 1 weighs about 0.1.

## The F2 overlay

F2 draws Rapier's geometry: green dynamic, orange kinematic, blue fixed (and bare colliders), yellow triggers, magenta joints, red contact points. Sleeping bodies and disabled colliders are dimmed. F3 draws velocity vectors. Both are off when `disable_debug_keys` is set in `Ryo.toml`; `EngineBuilder::show_colliders(true)` turns outlines on at start.

## Hierarchy and scale

- Bodies may sit anywhere in the hierarchy. A collider attaches to the nearest body at or above its entity; a body under a parent writes its local `Transform` through the parent. Rotation is written back continuously.
- Colliders follow the entity's **world** scale, mirroring and rotation. A zero scale takes a collider out until it grows.
- **Squash and stretch belongs on a child sprite**, not on a body with a collider: scaling the body scales the hitbox.
- Put hitboxes, sprites and effects as children of the body entity; give the body the `RigidBody` and the colliders.

## Common patterns

### Collectibles

```rust
ctx.world.spawn((
  Transform::from_xy(x, y),
  Renderable::sprite(coin_sprite),
  Collider::circle(12.0).as_trigger(),
  Coin { value: 10 },
));

// in update
for hit in ctx.messages::<CollisionStarted>() {
  let Some(other) = hit.other(self.player) else { continue };
  if let Some(coin) = ctx.world.get::<Coin>(other) {
    self.score += coin.value;
    ctx.world.despawn(other);
  }
}
```

### Jumping

```rust
let grounded = ctx.physics().contacts(self.player).iter().any(|t| t.normal.y > 0.7);
if grounded
  && ctx.input.key_pressed(KeyCode::Space)
  && let Some(mut velocity) = ctx.world.get_mut::<Velocity>(self.player)
{
  velocity.dy = -420.0;
}
```

### Teleporting

Write the `Transform`; the body teleports on the next physics step. Call `ctx.skip_interpolation(entity)` if the entity is `Interpolated`, so it does not slide there.

## Pitfalls

- Queries and contact state see the world as the last step left it, not the writes you made this tick.
- Set `lock_rotation()` on characters, or they tip over on the first slope.
- Despawned entities can still appear in `CollisionStopped`; check the entity still exists before using it.
- Do not put a game's movement velocity in `Velocity` on a kinematic body (see above).
- Do not scale a body with a collider for a visual effect; scale a child sprite.
- A `Collider` without a `RigidBody` is fixed: it does not move when its `Transform` changes except by teleport.
- Forces last one step; call them every step.
