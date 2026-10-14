---
name: ryo-physics
description: Rapier2D physics integration - colliders, rigid bodies, collision messages.
metadata:
  version: 2.0.0
---

# Ryo2D Physics Specialist

You help users add Rapier2D physics to their games: rigid bodies, colliders and collision messages.

**For complete API signatures, see the `ryo-api` skill.**

## How it runs

An entity enters the simulation when it has `Transform` + `RigidBody` + (`Collider` or `GroupedCollider`). The engine does the rest in `FixedSet::Physics`, once per fixed step, after the scene's `update` and the scene's fixed systems:

1. Register bodies that gained a `RigidBody` or a collider, and bodies whose entity was re-enabled.
2. Unregister bodies whose `RigidBody` went away (despawn included) and bodies whose entity was disabled.
3. Sync to Rapier: a `Transform` or `Velocity` the game changed is pushed in, compared by value with the body's, so untouched bodies stay asleep.
4. Step by the fixed timestep (`EngineBuilder::fixed_hz`, default 60 Hz). Slow motion runs fewer steps; pause and hitstop run none.
5. Sync back: awake dynamic bodies write `Transform.translation` and `Velocity`. These are ordinary component changes, so propagation and rendering see the body move.
6. Write `CollisionStarted` / `CollisionStopped` messages.

Physics pauses while the top scene's `is_physics_paused()` returns `true`, and while a pushed scene on top of another does not return `true` from `updates_below()`.

Units are pixels (100 pixels = 1 Rapier metre, converted internally). +Y is down.

## Rules

- **Bodies are root entities.** Physics reads and writes the local `Transform` as if it were world space, so a `RigidBody` on an entity with a `ChildOf` parent is not simulated: the engine logs it once and leaves it out until the entity is detached. Put the body on the root and hang sprites, hitboxes and effects off it as children.
- **Edit a `RigidBody` freely.** Changing a body's fields (say, `is_kinematic = true` when a character dies, or `gravity_scale` for a variable jump) updates the simulated body in place; contacts and sleep survive.
- **Rotation is locked.** Bodies never rotate, and `Transform.rotation` is not synced either way. Rotate a child sprite for looks.
- **Colliders are centred on the entity origin**, plus `Collider.offset`. With the default `Anchor::Center` that is the sprite's centre, so a 32x32 sprite takes `Collider::box_collider(32.0, 32.0)` with no offset.
- **Scale does not resize colliders.** Sizes are world pixels.
- **Writes in `update` reach Rapier the same step**; positions you read in `update` are from the previous step.
- **`Velocity` moves only physics bodies.** On an entity without `RigidBody` nothing integrates it.
- **Disabled bodies** (`Disabled`, used by `Pool`) leave the simulation and come back when enabled.

## Bodies

```rust
// Dynamic: gravity, forces and collision response
let player = ctx
  .world
  .spawn((
    Transform::from_xy(100.0, 100.0),
    Velocity::zero(),
    RigidBody::new().with_ccd(),
    Collider::box_collider(24.0, 32.0).with_friction(0.0),
  ))
  .id();

// Kinematic: moved by the game; it moves to its Transform over the next step
let platform = ctx
  .world
  .spawn((
    Transform::from_xy(200.0, 400.0),
    RigidBody::kinematic(),
    Collider::box_collider(100.0, 20.0),
  ))
  .id();

// Walls and floors: there is no static body type, so use a kinematic body
// that never moves. A Collider without a RigidBody is never simulated.
ctx.world.spawn((
  Transform::from_xy(400.0, 575.0),
  RigidBody::kinematic(),
  Collider::box_collider(800.0, 50.0),
));
```

`RigidBody` fields: `gravity_scale`, `drag` (linear damping), `is_kinematic`, `ccd_enabled`; builders `with_gravity_scale`, `with_drag`, `with_ccd`.

## Colliders

```rust
Collider::circle_collider(16.0)                // radius
Collider::box_collider(32.0, 64.0)             // width, height (axis-aligned)
Collider::circle_collider(32.0).as_trigger()   // sensor: messages, no response
Collider::box_collider(20.0, 8.0).with_offset(12.0, 0.0) // hitbox to the right
Collider::box_collider(32.0, 32.0).with_restitution(0.9).with_friction(0.1)
```

- `world_center(position)` and `world_bounds(position) -> (min, max)` give the shape where physics simulates it, for an origin at `position` (use `GlobalTransform::translation()`).
- `layer` / `mask` (`with_layer`, `with_mask`, `should_collide_with`) are data for your own checks; the simulation does not filter by them.

### Composite tiles

`GroupedCollider` covers a `CompositeTile` with one box. Its `offset_x`/`offset_y` is the box's **top-left** relative to the entity origin, the way composite parts are laid out:

```rust
let (composite, collider) = CompositeTileBuilder::new()
  .cap(cap_sprite, 0.0, 0.0)
  .body_tiled(body_sprite, 0.0, 32.0, TileDirection::Down, 128.0)
  .build_with_collider();

ctx.world.spawn((
  Transform::from_xy(100.0, 50.0),
  RigidBody::kinematic(),
  composite,
  collider,
));
```

## Collision messages

`CollisionStarted`, `CollisionStopped` and `ContactForce` have `a`, `b` (the colliders), `body_a`, `body_b` (their bodies; a collider with no body is its own), `involves(entity)` (collider or body), `other(entity) -> Option<Entity>` (the other body) and `other_collider(entity)`. `CollisionStarted` and `CollisionStopped` carry `trigger`; `CollisionStarted` also `contact: Option<Contact>` (`point` and `normal`, the normal pointing from `a` to `b`; `None` for triggers). `ContactForce { force, direction, started }` is sent every step a contact pushes above its collider's `with_force_events` threshold. `JointBroken { joint, a, b }` is registered for joints that break. The order of `a` and `b` is unspecified. A scene reads each message once with `ctx.messages::<T>()`:

```rust
fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
  for hit in ctx.messages::<CollisionStarted>() {
    if let Some(other) = hit.other(self.player)
      && let Some(hazard) = ctx.world.get::<Hazard>(other)
    {
      self.health -= hazard.damage;
    }
  }
  for _left in ctx.messages::<CollisionStopped>() {
    // leaving a trigger zone
  }
  SceneAction::Continue
}
```

A contact from step N is read in step N+1's `update`, together with the positions physics produced. Systems read them with `MessageReader<CollisionStarted>`. No contact message is missed at any frame rate (buffers advance only after a fixed step); the `ryo-messages` skill has the details.

## The physics world and settings

Both are resources:

```rust
// Gravity in pixels per second squared (default (0, 980))
EngineBuilder::new().insert_resource(PhysicsConfig::default().zero_gravity());
// or at runtime
if let Some(mut config) = ctx.resource_mut::<PhysicsConfig>() {
  config.gravity = Vec2::new(0.0, 1600.0);
}

// Current contacts between solid colliders, from Rapier's narrow phase
// (a trigger's overlaps arrive as messages)
if let Some(physics) = ctx.resource::<Physics>() {
  let touching = physics.are_intersecting(self.player, self.platform);
  let all = physics.get_current_intersections(self.player);
  let asleep = physics.is_sleeping(self.player);
}
```

`Physics`'s own event list (`get_collision_events`, `has_collision`) is drained into the messages inside the physics set, so it is empty when scenes run; read the messages instead.

## Common patterns

### Collectibles

```rust
#[derive(Component)]
struct Coin {
  value: u32,
}

ctx.world.spawn((
  Transform::from_xy(x, y),
  Renderable::sprite(coin_sprite),
  RigidBody::kinematic(),
  Collider::circle_collider(12.0).as_trigger(),
  Coin { value: 10 },
));

// in update
for hit in ctx.messages::<CollisionStarted>() {
  let Some(other) = hit.other(self.player) else {
    continue;
  };
  if let Some(coin) = ctx.world.get::<Coin>(other) {
    self.score += coin.value;
    ctx.world.despawn(other);
  }
}
```

### Jumping

```rust
if ctx.input.key_pressed(KeyCode::Space)
  && let Some(mut velocity) = ctx.world.get_mut::<Velocity>(self.player)
{
  velocity.dy = -420.0;
}
```

### Teleporting

Write the `Transform`; the body teleports on the next physics step. Call `ctx.skip_interpolation(entity)` if the entity is `Interpolated`, so it does not slide there.

## Debugging

F2 toggles collider outlines, drawn where physics simulates them; F3 draws velocity vectors (unless `disable_debug_keys` is set in `Ryo.toml`). `EngineBuilder::show_colliders(true)` turns outlines on at start.
