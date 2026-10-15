---
name: ryo-character
description: Characters - the character controller, platformer and top-down movement presets, MoveIntent and key bindings, one-way platforms, triggers with pickups, kill zones, checkpoints and respawns, tile colliders and knockback.
metadata:
  version: 1.0.0
---

# Ryo2D Characters

You help users make things that walk, run, jump and dash: players, enemies and NPCs, the levels they move through and the zones they collect, die and respawn in.

**For complete API signatures, see the `ryo-api` skill.** Bodies, colliders, queries and messages in general: `ryo-physics`.

## Which to use

| You want | Use |
|---|---|
| A side-view player with the usual feel (acceleration, coyote time, jump buffer, variable jump, optional air jumps, wall slide and jump, dash) | `PlatformerMovement` + `MoveBindings` |
| A top-down player (eight directions, diagonals no faster, optional dash) | `TopDownMovement` + `MoveBindings` |
| An enemy or NPC that moves like a player | the same preset, with your system writing its `MoveIntent` (no bindings) |
| Movement no preset covers (a charge jump, a grapple, a vehicle, a rolling ball) | the bare `CharacterController`, with your code writing `Velocity` |
| A preset plus a rule of your own (a spring pad, a launch, a bounce off an enemy) | the preset, and write `Velocity` in `update`: the preset starts each step from it |

Every preset brings the controller (`CharacterController::platformer()` or `top_down()`), its state and a `MoveIntent`, so a player is a `Collider`, a preset and bindings:

```rust
ctx.world.spawn((
  Transform::from_xy(100.0, 200.0),
  Collider::rect(14.0, 26.0),
  PlatformerMovement::default(),
  MoveBindings::default(),
));
```

## The character controller

`CharacterController` makes its entity a character: a kinematic body (added when there is none, rotation locked) that the engine sweeps by its `Velocity` once per fixed step, sliding along what it hits. The `Collider` on the same entity is the swept shape. It collides with what that collider's layer and mask allow; triggers never block it. Characters block each other (each is a body the others' sweeps meet); give them layers and masks that skip each other to let them pass.

| Field | Builder | Default | Meaning |
|---|---|---|---|
| `mode: CharacterMode` | `CharacterController::platformer()`, `top_down()` | `Platformer` | `Platformer`: up is -Y, floors, walls and ceilings, snapping and steps. `TopDown`: every surface is a wall it slides along; no floor, ceiling, snapping or steps |
| `max_slope: f32` (radians) | `with_max_slope(degrees)` | 45° | the steepest floor; steeper surfaces are walls and it slides down them |
| `step_height: Option<f32>` | `with_step_height(px)` | off | walk up ledges this high (platformer) |
| `snap_to_ground: Option<f32>` | `with_snap_to_ground(px)` | a fifth of the collider's height; 0 turns it off | keep to the ground down slopes and over bumps (platformer; never on the way up) |
| `skin: Option<f32>` | `with_skin(px)` | 1% of the collider's height | the gap kept from surfaces |
| `push: bool` | `with_push(bool)` | on | push dynamic bodies it walks into (its mass from its collider's density); off, they stop it like walls |
| `drop_through: bool` | | off | drop through the one-way platform underfoot (see below) |

A preset inserts its controller only when the entity has none, so put your own in the bundle to change its settings: `(CharacterController::platformer().with_step_height(6.0), PlatformerMovement::default(), ..)`.

### The `Velocity` contract

- **In:** `Velocity` is what the game wants this step, in px/s: its gravity, run and jump (or a preset's). Characters are kinematic, so `PhysicsConfig::gravity` never reaches them: add gravity yourself or use a preset.
- **When:** write it in the fixed step, in `update` or a `Phase::FixedUpdate` system. Those run before the physics step (`FixedSet::Movement` sits between the game's systems and physics, and the presets run there).
- **Out:** after the step it is the real velocity: landing zeroes the fall, a head bump stops the rise, a wall takes out what ran into it, and a slope too steep to climb stops the run rather than turning it into a climb. Standing on a gentle slope does not creep. Leaving a moving floor adds its velocity (momentum). So next step starts from what happened.
- A non-finite `Velocity` is not applied: the character stays, its velocity becomes zero (logged once).
- Knockback is swept with it but never added to it (see Knockback).

### `CharacterState`

Written by the engine every step; reading it in `update` shows the last step's move. Normals point out of the surface toward the character: flat ground is `(0, -1)`, a wall on the right `(-1, 0)`.

| Field | |
|---|---|
| `on_floor`, `on_wall`, `on_ceiling` | what the move touched. A wall is reported only on a step the character moves into it |
| `floor_normal`, `wall_normal` | zero when not touching one |
| `floor: Option<Entity>` | the body (or bare collider) underfoot |
| `floor_one_way` | the floor is a one-way platform |
| `platform_velocity` | how fast the floor moves where it stands, px/s (zero off the floor) |
| `landed`, `left_floor` | true for the one step it lands / leaves the floor |
| `sliding_down_slope` | on a slope steeper than `max_slope`, sliding down |
| `velocity_before` | the `Velocity` the step asked for, before anything clipped it (bounces) |
| `hits: Vec<CharacterHit>` | everything the move touched: `CharacterHit { entity, body, point, normal }` (the collider, its body, the world point, the normal); a collider can be listed more than once |

Top-down characters report only walls. For contacts as messages (`CollisionStarted` between a character and what it touches) see `ryo-physics`; `hits` is usually simpler.

### Teleporting

Writing a character's `Transform` teleports it: it is put there without a sweep, its `CharacterState` resets (in the air, touching nothing, any drop-through over), and it still moves by its `Velocity` from the new pose in that step, pushing out of whatever it was put into first. A rotation written with it is kept. So a respawn onto a floor reports `landed` on that step, and a correction mid-flight costs no step of motion. A write that leaves the pose where it was does not count. Zero `Velocity` too if it should arrive standing still, and call `ctx.skip_interpolation(entity)` if it is `Interpolated`.

### Riding and pushing

- What it stands on carries it: a kinematic platform moved by `Transform` or by `Velocity` (turning ones too) or a dynamic body; fixed bodies do not move. Walls and ceilings still stop the ride. A lift carries its rider up through a one-way platform above it.
- Leaving the floor (a jump, walking off, the platform despawned or disabled) adds `platform_velocity` to `Velocity`. A jump off a rising platform goes higher.
- A floor that is despawned or disabled is gone at once: the character falls on the next step.
- With `push`, dynamic bodies it walks into are pushed, and one it stands on is weighed down.

### Limits and warnings

- Logged once per entity and skipped: a controller on a dynamic (or fixed) body, a controller without a `Collider` on its own entity, a non-finite `Velocity`.
- Spawned overlapping something solid, a character is pushed out within a few steps. Tile outlines are hollow, though: one that ends up wholly inside a solid region is not pushed out (see Tile colliders).
- The sweep is continuous: a fast character does not tunnel through a thin wall, and a fast fall lands on a one-way segment.
- Very large colliders lose sub-pixel precision: a run along a floor 4000 px or longer in one piece (a tile map's straight run is one edge) can sink into it by up to 0.16 px at times.
- While physics is paused (`is_physics_paused`, or a pushed scene that does not `updates_below`) characters do not move.

## One-way platforms

```rust
Collider::rect(96.0, 8.0).one_way()                     // solid only from above
Collider::segment(Vec2::ZERO, Vec2::new(96.0, 0.0)).one_way()
Collider::rect(8.0, 96.0).one_way_towards(Vec2::NEG_X)  // stops what comes from the left
```

- The direction is in the collider's own space, so it turns with the collider. A zero direction is solid from every side (logged once).
- A character is held only when it starts a move entirely on the solid side (within 1 px): it jumps up through, walks in from the sides, and lands once clear. Keep the solid side flat across the direction: tilt a platform by turning the collider (`with_rotation` or the entity's rotation), not by sloping a polygon's top.
- Dynamic bodies meet one-way colliders through the solver's contacts: they pass from below and rest on top.
- **Drop-through:** set `controller.drop_through = true` while standing on a one-way floor (`state.floor_one_way`). Every one-way collider is then ignored until the character is fully below the one it dropped from, lands on something solid, is teleported, or that platform goes (despawned, or no longer one-way); the engine clears the flag then. Set anywhere else (solid ground, the air), the flag clears at once and nothing is ignored. Platforms stacked closer than the character's height are dropped through together. The platformer preset drops on down + jump.

## Movement input: `MoveIntent` and `MoveBindings`

`MoveIntent` is what every preset reads:

| Field | |
|---|---|
| `direction: Vec2` | where to move, -1 to 1 per axis, +Y down, analog from sticks |
| `jump_held: bool` | jump is held (releasing it early cuts a jump short) |
| `jump_pressed: bool`, `dash_pressed: bool` | one-shots: the presets act on them and clear them |

- `direction` is not clamped when written; the presets clamp it as they read it. `clamped_direction()` limits its length to 1 (the top-down preset: a diagonal is no faster, half a stick is half speed). `clamped_axes()` limits each axis to -1..1 on its own (the platformer: holding up or down while running keeps full speed). Both read a non-finite value as zero.
- **Writing it yourself** (AI, networking, replays, cutscenes): set `direction` every step, and set a one-shot on the step it happens; the preset clears it once acted on. Write it in `update` or a `FixedUpdate` system; the preset reads it the same step.

`MoveBindings` (brings a `MoveIntent`) fills the intent from the keyboard and the first gamepad every fixed step, before the presets. A press is seen once, in the first fixed step after it.

| Field | Default | Builder |
|---|---|---|
| `left`, `right`, `up`, `down` | A/←, D/→, W/↑, S/↓ | `with_left(keys)`, `with_right`, `with_up`, `with_down` |
| `jump` | Space | `with_jump(keys)` |
| `dash` | left and right Shift | `with_dash(keys)` |
| `gamepad` | on: left stick and d-pad move, South jumps, West dashes | `keyboard_only()` |

Bindings overwrite the whole intent every step, so a game that drives an entity's intent itself (a cutscene taking over the player) removes `MoveBindings` first. Two local players: give each its own keys and `keyboard_only()` on one.

## `PlatformerMovement`

Brings `MoveIntent`, `PlatformerState` and `CharacterController::platformer()` (a top-down controller already there is warned about once and the preset skips it). It writes `Velocity` every step, its own gravity included, starting from the real velocity the last move left.

| Field | Default | Builder |
|---|---|---|
| `run_speed`, `accel_time`, `decel_time` | 180 px/s, 0.08 s, 0.06 s (0 is instant) | `with_run(speed, accel_time, decel_time)` |
| `air_control` | 0.65 (share of the ground acceleration in the air; 0 keeps the speed) | `with_air_control(fraction)` |
| `jump_height`, `time_to_apex` | 64 px, 0.38 s | `with_jump(height, time_to_apex)` |
| `fall_multiplier`, `max_fall_speed` | 1.5, 600 px/s | `with_fall(multiplier, max_speed)` |
| `coyote_time` | 0.1 s (a jump still works this long after leaving a floor, or a wall) | `with_coyote_time(seconds)` |
| `jump_buffer` | 0.1 s (a jump pressed this long before landing is kept) | `with_jump_buffer(seconds)` |
| `jump_cut` | 0.5 (the rise kept when jump is released early; 1 always jumps full height) | `with_jump_cut(factor)` |
| `knockback_control` | 0 (no steering while knocked back) | `with_knockback_control(fraction)` |
| `air_jumps` | 0 | `with_air_jumps(count)` |
| `wall_slide: Option<f32>` | off (fastest fall while pressing into a wall in the air) | `with_wall_slide(max_speed)` |
| `wall_jump: Option<WallJump>` | off (`WallJump { launch: Vec2, lock_time }`: speed away from the wall and up, both positive, and how long the run input is ignored after) | `with_wall_jump(launch, lock_time)` |
| `dash: Option<Dash>` | off (`Dash { speed, duration, cooldown }`) | `with_dash(speed, duration, cooldown)` |

- **The jump** is set by its height and the time to its top: gravity and launch speed come from them, exact at the fixed step (the defaults give about 927 px/s² and 352 px/s at 60 Hz). That gravity is the preset's only gravity, so `jump_height` 0 means no gravity at all.
- **Each step:** a landing writes `Landed` and refills the air jumps and the air dash. A dash (`dash_pressed`, cooldown over, the air dash unused in the air) goes along the input, or the facing when there is none, with no gravity or steering while it lasts, and leaves no faster than `run_speed`. Otherwise the run moves toward `direction.x * run_speed`; then, the first that applies: down + jump on a one-way floor drops through it; a buffered jump with coyote time left jumps; in the air, a buffered jump within `coyote_time` of touching a wall wall-jumps (away from it, the run held off for `lock_time`, the air jumps refilled); a press in the air with an air jump left air-jumps. Releasing jump while rising cuts the rise by `jump_cut`, once. Gravity is added (times `fall_multiplier` falling, capped at `max_fall_speed`), and a wall slide caps the fall while pressing into a wall in the air.
- Wall jumps and slides need the input (or the momentum) pressing into the wall: the controller reports a wall only when the move runs into it.
- **Change settings at runtime** by writing the component (`ctx.world.get_mut::<PlatformerMovement>(player)`): a power-up's extra air jump, slower running in water.
- **A launch of your own** (spring pad, explosion, bounce off an enemy): write `Velocity` in `update`; the preset carries on from it, adding gravity and steering toward the input. Such a launch is not cut short by releasing jump (only the preset's own jumps are).

`PlatformerState` (engine-written): `facing` (-1 or 1), `jumping` (rising from a jump), `air_jumps_left`, `wall_sliding`, `dashing`, `dash_ready` (the air dash is unused). `reset()` forgets jumps, timers and dashes, keeping the facing (a kill zone's respawn calls it).

## `TopDownMovement`

Brings `MoveIntent`, `TopDownState` and `CharacterController::top_down()`. It writes `Velocity` every step: toward `clamped_direction() * max_speed` in a straight line, so diagonals are no faster and half a stick is half speed.

| Field | Default | Builder |
|---|---|---|
| `max_speed`, `accel_time`, `decel_time` | 160 px/s, 0.08 s, 0.1 s (0 is instant) | `with_speed(max_speed, accel_time, decel_time)` |
| `dash: Option<Dash>` | off; dashes along the facing, ignoring the input while it lasts | `with_dash(speed, duration, cooldown)` |
| `knockback_control` | 0 | `with_knockback_control(fraction)` |

`TopDownState`: `facing` (unit length, the last direction steered; `(0, 1)` until steered), `dashing`; `reset()`. Characters get no gravity, but dynamic props do: a top-down game sets `PhysicsConfig::default().zero_gravity()` (or gives props `with_gravity_scale(0.0)` and some `with_linear_damping`).

## `MovementEvent`

`MovementEvent { entity, kind }`, `kind` one of `MovementEventKind::{Jumped, AirJumped, WallJumped, Dashed, Landed}` (non-exhaustive: match with a `_` arm). The presets write it in `FixedSet::Movement`, after `update`, so `update` reads it on the next step. Use it for sounds, dust and squash.

```rust
for event in ctx.messages::<MovementEvent>() {
  if event.entity == player && event.kind == MovementEventKind::Landed {
    SquashStretch::apply(ctx.world, sprite, 0.3); // the sprite is a child of the player
  }
}
```

## Triggers

A trigger collider (any shape with `.as_trigger()`) detects overlaps without pushing; characters and other moving bodies set triggers off (two fixed colliders never meet). The engine sends:

- `TriggerEntered { trigger, other, other_body }` and `TriggerExited { .. }`: `trigger` is the trigger collider's entity, `other` the collider that entered or left, `other_body` its body (the collider itself when it has none). `actor()` returns `other_body`: use it for "who". One message per body: a body with several colliders enters on its first and exits when its last leaves. On `TriggerExited` either entity may be gone.
- `TriggerContents` on every trigger (added after its first step): the bodies inside now, in the order they entered: `entities()`, `contains(e)`, `len()`, `is_empty()`.
- What can set a trigger off is the usual layer and mask test, both ways.
- Messages are written after the physics step, so `update` reads them on the next step.

```rust
// A pressure plate: open the door while anything stands on it
let pressed = ctx.world.get::<TriggerContents>(plate).is_some_and(|inside| !inside.is_empty());
```

## Behaviours: `Pickup`, `KillZone`, `Checkpoint`, `Respawn`

Put `Pickup`, `KillZone` or `Checkpoint` on an entity with a trigger collider (a non-trigger is warned about once and never fires); limit what sets it off with the trigger's layer and mask. They act on the previous step's `TriggerEntered` messages in `FixedSet::Movement`, one step after the overlap, and keep running while physics is paused, so an entry just before a pause is not lost.

- **`Pickup::new()`**: collected by the first thing that enters (two collectors in one step collect it once): `PickedUp { pickup, by }` is written and the pickup despawned. `.respawn_after(seconds)` disables it instead (`Disabled`, so it is neither drawn nor simulated) and brings it back after that many seconds of fixed steps (the time waits while physics is paused); something still standing on it then collects it again at once.
- **`KillZone::new()`**: writes `Killed { entity, zone }` for each entrant. An entrant with `Respawn` is put back at its point: `Transform.translation` set (a teleport), `Velocity` zeroed, `Knockback::stop()`, `PlatformerState` / `TopDownState` reset. Others only get the message, unless the zone is `.despawning()` (bullets, enemies falling out of the level).
- **`Checkpoint::new()`**: moves the entrant's `Respawn::point` to the checkpoint's position plus `.with_offset(v)` and writes `CheckpointReached { checkpoint, entity }` when the point moved (more than half a pixel).
- **`Respawn::here()`** takes the entity's `Transform` the first time the engine sees it; **`Respawn::at(point)`** a given world point. `point` is a public field: set it yourself for a respawn of your own (`transform.translation = respawn.point`). Put it on root entities: it is written to `Transform.translation`.

Checkpoints have no order: entering a lower one moves `Respawn::point` back to it. When checkpoints should only go up, keep your own record and put the point back. Read it first in `update`, before anything of yours respawns from `Respawn::point`:

```rust
/// A checkpoint's place in the level: higher is further on.
#[derive(Component)]
struct CheckpointOrder(u32);

#[derive(Default)]
struct Progress {
  best: Option<(u32, Vec2)>,
}

impl Progress {
  /// Keep the furthest checkpoint: a lower one gets the furthest's point back.
  fn read_checkpoints(&mut self, ctx: &mut SceneContext) {
    for reached in ctx.messages::<CheckpointReached>() {
      let Some(order) = ctx.world.get::<CheckpointOrder>(reached.checkpoint).map(|o| o.0) else {
        continue;
      };
      let Some(mut respawn) = ctx.world.get_mut::<Respawn>(reached.entity) else {
        continue;
      };
      match self.best {
        Some((best, point)) if order <= best => respawn.point = point,
        _ => self.best = Some((order, respawn.point)),
      }
    }
  }
}
```

A kill-zone respawn does not skip interpolation: an `Interpolated` character can be drawn partway for one frame.

## Tile colliders

`TileColliders` turns a grid of cells into colliders: seamless outlines around solid and slope cells (a long floor is one edge, so nothing snags on seams) and a one-way segment along the top of each run of one-way cells.

```rust
let mut tiles = TileColliders::new(40, 12, Vec2::splat(16.0)); // width, height in cells, cell size
tiles.set(3, 11, TileCell::Solid);
let level = ctx.world.spawn((Transform::from_xy(0.0, 0.0), tiles)).id();
```

- `TileCell::{Empty, Solid, OneWay, Slope(SlopeCorner)}`; `SlopeCorner::{BottomLeft, BottomRight, TopLeft, TopRight}` is the filled corner of a 45° half-cell (`BottomRight` rises to the right). `SlopeCorner::from_name("bottom-left")` reads Tiled's spelling.
- `TileColliders::new(width, height, tile_size)`, `from_cells(width, height, tile_size, cells)` (row by row from the top; missing cells are empty), `get(x, y)`, `set(x, y, cell) -> bool` (false, and nothing changes, outside the grid or when the cell already is that), `width()`, `height()`. Fields and builders: `tile_size`, `layer` / `with_layer`, `mask` / `with_mask`, `friction` / `with_friction`, `restitution` / `with_restitution`, `solid_rects` / `solid_rects()`.
- Its `Transform` places cell (0, 0)'s top-left; +Y is down. With no body the colliders are fixed; with a kinematic `RigidBody` on the map entity the whole map moves.
- **Outlines are hollow**: a body that ends up inside a solid region is not pushed out. `solid_rects()` builds merged rectangles instead (solid interiors, but seams between rectangles).
- **Editing**: `ctx.world.get_mut::<TileColliders>(level)` and `set`. Only a real change rebuilds, all of the map's colliders at once, before that step's physics. Disabling the map entity removes its colliders and enabling it rebuilds them from the cells as they are then; removing the component removes them. A grid over 8192 × 8192 cells is refused (logged, empty), and so is a non-positive tile size.
- **Which entity:** the colliders are child entities of the map. With no body on the map, collision messages and `CharacterState::floor` name such a part: follow `ChildOf` to the map (`ctx.world.get::<ChildOf>(part).map(|c| c.parent())`). A rebuild makes new parts, so it re-sends `CollisionStopped` / `CollisionStarted` for what touched the map, and `floor` names the new part.
- From a Tiled map: `tilemap.tile_colliders()` reads tile properties (`solid`, `platform` for one-way, a `slope` custom property naming the corner); spawn it with a `Transform` at the map's origin.
- Navigation: `NavigationGrid::rebuild_from_world` blocks a map's solid and slope cells exactly (one-way cells stay walkable) and skips its outline colliders; by hand, `grid.block_tiles(&tiles, origin)`. Only the map's position counts, not its rotation or scale.

## Knockback

`Knockback::apply(ctx.world, entity, direction, force)` pushes in a direction with a force (px/s) that decays (see `ryo-animation`). What the push does depends on the target:

| Target | Effect |
|---|---|
| A character | swept with its move as extra velocity, never added to `Velocity`: walls, floors and ceilings take out the part running into them, and a push straight into a wall stops. The presets steer at `knockback_control` (0: none) while it lasts |
| A dynamic body | each hit is one change in velocity of its own, `direction * force`, whatever the mass; physics slows it from there |
| A kinematic body without a controller, a fixed body | ignored, logged once |
| No body | `Transform` moves by the current knockback velocity |

The knockback system runs in `FixedSet::Movement`, after `update`: a hit applied in `update` acts in that step's physics, and `KnockbackStarted` / `KnockbackFinished` reach `update` the next step. `Knockback::stop()` ends it and drops a hit not yet given to a dynamic body; a kill zone's respawn calls it.

## One fixed step

```
Scene::update, then the scene's FixedUpdate systems     your Velocity / MoveIntent / Transform writes
FixedSet::Movement                                       zone behaviours (last step's triggers), tile rebuilds,
                                                         bindings -> MoveIntent, knockback, presets -> Velocity
FixedSet::Physics                                        characters swept, bodies stepped, messages written
```

`CharacterState`, `PlatformerState`, trigger and collision messages, `MovementEvent`, `PickedUp`, `Killed` and `CheckpointReached` all describe the step before the `update` that reads them. Under a pause (`PhysicsPaused`) the bindings, presets, knockback and pickup respawn timers wait too; the zone behaviours keep draining the messages that arrived before it.

## Recipes

Each recipe is complete: it compiles as it stands against the prelude (a game adds its `main`, as the top-down one shows).

### A platformer player with bindings

```rust
use ryo2d::prelude::*;

#[derive(Default)]
struct Level {
  player: Option<Entity>,
  sprite: Option<Entity>,
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Ground (a bare collider is fixed) and a shelf to jump up through
    ctx.world.spawn((Transform::from_xy(400.0, 580.0), Collider::rect(800.0, 40.0)));
    ctx.world.spawn((Transform::from_xy(520.0, 500.0), Collider::rect(160.0, 12.0).one_way()));

    let look = ctx
      .assets
      .create_sprite(Sprite::solid_rect(14, 26, 240, 240, 255, 255), "player");
    let player = ctx
      .world
      .spawn((
        Transform::from_xy(200.0, 540.0),
        Interpolated,
        Collider::rect(14.0, 26.0),
        // Brings CharacterController::platformer(), MoveIntent and PlatformerState
        PlatformerMovement::default()
          .with_run(200.0, 0.08, 0.06)
          .with_jump(72.0, 0.36)
          .with_air_jumps(1),
        // WASD and the arrows move, Space jumps, Shift dashes, plus the first gamepad
        MoveBindings::default(),
      ))
      .id();
    // The sprite is a child, so squash and stretch leave the hitbox alone
    self.sprite = Some(ctx.spawn_child(player, (Transform::IDENTITY, Renderable::sprite(look))));
    self.player = Some(player);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let (Some(player), Some(sprite)) = (self.player, self.sprite) else {
      return SceneAction::Continue;
    };
    for event in ctx.messages::<MovementEvent>() {
      if event.entity != player {
        continue;
      }
      match event.kind {
        MovementEventKind::Landed => SquashStretch::apply(ctx.world, sprite, 0.3),
        MovementEventKind::Jumped | MovementEventKind::AirJumped => {
          SquashStretch::apply_x(ctx.world, sprite, 0.2)
        }
        _ => {}
      }
    }
    if let Some(at) = ctx.world.get::<Transform>(player).map(|t| t.translation) {
      ctx.camera().follow(at, 0.15);
    }
    SceneAction::Continue
  }
}
```

### A top-down player

```rust
use ryo2d::prelude::*;

#[derive(Default)]
struct Dungeon {
  coins: u32,
}

impl Scene for Dungeon {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // A walled room of 16 px tiles
    let mut room = TileColliders::new(40, 30, Vec2::splat(16.0));
    for x in 0..40 {
      room.set(x, 0, TileCell::Solid);
      room.set(x, 29, TileCell::Solid);
    }
    for y in 0..30 {
      room.set(0, y, TileCell::Solid);
      room.set(39, y, TileCell::Solid);
    }
    ctx.world.spawn((Transform::IDENTITY, room));
    // A crate to push, and coins
    ctx.world.spawn((
      Transform::from_xy(320.0, 240.0),
      RigidBody::dynamic().with_linear_damping(8.0).lock_rotation(),
      Collider::rect(20.0, 20.0),
    ));
    for x in [120.0, 400.0, 560.0] {
      ctx.world.spawn((
        Transform::from_xy(x, 120.0),
        Collider::circle(6.0).as_trigger(),
        Pickup::new(),
      ));
    }
    ctx.world.spawn((
      Transform::from_xy(200.0, 240.0),
      Collider::circle(9.0),
      TopDownMovement::default()
        .with_speed(180.0, 0.06, 0.08)
        .with_dash(420.0, 0.15, 0.6),
      MoveBindings::default(),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.coins += ctx.messages::<PickedUp>().len() as u32;
    SceneAction::Continue
  }
}

fn main() {
  EngineBuilder::new()
    .title("Dungeon")
    // Characters never get gravity; this keeps the crate on the floor plan too
    .insert_resource(PhysicsConfig::default().zero_gravity())
    .start_scene(Dungeon::default())
    .build()
    .run()
    .unwrap();
}
```

### An enemy that patrols by writing `MoveIntent`

```rust
use ryo2d::prelude::*;

/// Walks one way until it meets a wall or a ledge, then turns round.
#[derive(Component)]
struct Patrol {
  facing: f32,
}

/// Runs in the fixed step before the presets, so they read this step's intent.
fn patrol(
  physics: Res<Physics>,
  mut enemies: Query<(Entity, &mut Patrol, &mut MoveIntent, &CharacterState, &GlobalTransform)>,
) {
  for (entity, mut patrol, mut intent, state, global) in &mut enemies {
    // A wall ahead: its normal points back at the enemy
    if state.on_wall && state.wall_normal.x * patrol.facing < 0.0 {
      patrol.facing = -patrol.facing;
    }
    // A ledge ahead: no ground below a point just past its front edge
    if state.on_floor {
      let ahead = global.translation() + Vec2::new(patrol.facing * 12.0, 0.0);
      let filter = QueryFilter::new().excluding(entity);
      if physics.raycast(ahead, Vec2::Y, 24.0, &filter).is_none() {
        patrol.facing = -patrol.facing;
      }
    }
    intent.direction = Vec2::new(patrol.facing, 0.0);
  }
}

struct Arena;

impl Scene for Arena {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.spawn((Transform::from_xy(400.0, 580.0), Collider::rect(800.0, 40.0)));
    ctx.world.spawn((Transform::from_xy(300.0, 420.0), Collider::rect(200.0, 16.0)));
    for x in [150.0, 300.0, 600.0] {
      ctx.world.spawn((
        Transform::from_xy(x, 400.0),
        Collider::rect(16.0, 20.0),
        // The preset reads MoveIntent; no MoveBindings, so only `patrol` writes it
        PlatformerMovement::default().with_run(60.0, 0.1, 0.1),
        Patrol { facing: 1.0 },
      ));
    }
    ctx
      .add_systems(Phase::FixedUpdate, patrol)
      .expect("scenes may add FixedUpdate systems");
  }
}
```

To make it jump, set `intent.jump_pressed = true` on the step it should (the preset clears it).

### Custom movement on the bare controller: a charge jump with a wall bounce

```rust
use ryo2d::prelude::*;

const GRAVITY: f32 = 1400.0;
const MAX_CHARGE: f32 = 0.8;
const LEAP: f32 = 720.0;
const BOUNCE: f32 = 0.7;

/// Hold Space on the ground to charge, release to leap the way A or D
/// points; walls bounce the leap back.
#[derive(Default)]
struct Leaper {
  player: Option<Entity>,
  charge: f32,
}

impl Scene for Leaper {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.spawn((Transform::from_xy(400.0, 580.0), Collider::rect(800.0, 40.0)));
    for x in [10.0, 790.0] {
      ctx.world.spawn((Transform::from_xy(x, 300.0), Collider::rect(20.0, 600.0)));
    }
    let player = ctx
      .world
      .spawn((
        Transform::from_xy(400.0, 540.0),
        CharacterController::platformer(),
        Collider::rect(14.0, 26.0),
      ))
      .id();
    self.player = Some(player);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    let Some(player) = self.player else {
      return SceneAction::Continue;
    };
    // What the last step's move found, and the real velocity it left
    let Some(state) = ctx.world.get::<CharacterState>(player).cloned() else {
      return SceneAction::Continue;
    };
    let Some(mut velocity) = ctx.world.get::<Velocity>(player).map(Velocity::as_vec2) else {
      return SceneAction::Continue;
    };
    let aim = f32::from(u8::from(ctx.input.key_down(KeyCode::KeyD)))
      - f32::from(u8::from(ctx.input.key_down(KeyCode::KeyA)));
    let mut leapt = false;
    if state.on_floor {
      velocity.x = 0.0;
      if ctx.input.key_down(KeyCode::Space) {
        self.charge = (self.charge + dt).min(MAX_CHARGE);
      } else if self.charge > 0.0 {
        let power = LEAP * (0.4 + 0.6 * self.charge / MAX_CHARGE);
        velocity = Vec2::new(aim * power * 0.55, -power);
        self.charge = 0.0;
        leapt = true;
      }
    } else if state.on_wall {
      // The wall took the run out of Velocity; bounce back what the step asked for
      velocity.x = -state.velocity_before.x * BOUNCE;
    }
    // Characters get no gravity from physics: add it
    if !leapt {
      velocity.y = (velocity.y + GRAVITY * dt).min(900.0);
    }
    if let Some(mut current) = ctx.world.get_mut::<Velocity>(player) {
      *current = Velocity::from_vec2(velocity);
    }
    SceneAction::Continue
  }
}
```

### A level from a string grid, with coins, spikes and checkpoints

```rust
use ryo2d::prelude::*;

const TILE: f32 = 16.0;
/// `#` solid, `=` one-way, `/` `\` slopes, `o` coin, `^` spikes, `F` checkpoint, `P` the start.
const LEVEL: &[&str] = &[
  "########################################",
  "#......................................#",
  "#......................................#",
  "#...................o.o.o..............#",
  "#..................=======.......o.....#",
  "#..........o..................######...#",
  "#........=====.........................#",
  "#..P...................................#",
  "#............./\\...................o...#",
  "#....F......./##\\..^^^....F.^^^........#",
  "########################################",
];

/// The world point at the bottom middle of a cell (the map sits at the origin).
fn feet(column: usize, row: usize) -> Vec2 {
  Vec2::new((column as f32 + 0.5) * TILE, (row + 1) as f32 * TILE)
}

#[derive(Default)]
struct Level {
  player: Option<Entity>,
  coins: u32,
  deaths: u32,
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let coin = ctx
      .assets
      .create_sprite(Sprite::solid_rect(8, 8, 255, 220, 60, 255), "coin");
    let width = LEVEL.iter().map(|row| row.len()).max().unwrap_or(0);
    let mut cells = Vec::with_capacity(width * LEVEL.len());
    for (row, line) in LEVEL.iter().enumerate() {
      for column in 0..width {
        let c = line.as_bytes().get(column).copied().unwrap_or(b'.');
        cells.push(match c {
          b'#' => TileCell::Solid,
          b'=' => TileCell::OneWay,
          b'/' => TileCell::Slope(SlopeCorner::BottomRight),
          b'\\' => TileCell::Slope(SlopeCorner::BottomLeft),
          _ => TileCell::Empty,
        });
        let at = feet(column, row);
        match c {
          b'o' => {
            ctx.world.spawn((
              Transform::from_translation(at - Vec2::new(0.0, TILE / 2.0)),
              Renderable::sprite(coin.clone()),
              Collider::circle(5.0).as_trigger(),
              Pickup::new(),
            ));
          }
          b'^' => {
            ctx.world.spawn((
              Transform::from_translation(at - Vec2::new(0.0, 4.0)),
              Collider::rect(TILE, 8.0).as_trigger(),
              KillZone::new(),
            ));
          }
          b'F' => {
            // A respawn stands the player on this cell's floor
            ctx.world.spawn((
              Transform::from_translation(at - Vec2::new(0.0, TILE)),
              Collider::rect(TILE, 2.0 * TILE).as_trigger(),
              Checkpoint::new().with_offset(Vec2::new(0.0, TILE - 12.0)),
            ));
          }
          b'P' => {
            let player = ctx
              .world
              .spawn((
                Transform::from_translation(at - Vec2::new(0.0, 12.0)),
                Collider::rect(12.0, 22.0),
                PlatformerMovement::default().with_jump(72.0, 0.38),
                MoveBindings::default(),
                Respawn::here(),
              ))
              .id();
            self.player = Some(player);
          }
          _ => {}
        }
      }
    }
    ctx.world.spawn((
      Transform::IDENTITY,
      TileColliders::from_cells(width as u32, LEVEL.len() as u32, Vec2::splat(TILE), cells),
    ));
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.coins += ctx.messages::<PickedUp>().len() as u32;
    for killed in ctx.messages::<Killed>() {
      if Some(killed.entity) == self.player {
        self.deaths += 1;
      }
    }
    for reached in ctx.messages::<CheckpointReached>() {
      game_info!("checkpoint {} reached", reached.checkpoint);
    }
    if let Some(at) = self
      .player
      .and_then(|player| ctx.world.get::<Transform>(player))
      .map(|t| t.translation)
    {
      ctx.camera().follow(at, 0.15);
    }
    SceneAction::Continue
  }
}
```

The coins are drawn by their sprite; a level with art draws its tiles separately (sprites per cell, or a `Tilemap` with `tile_colliders()`), since `TileColliders` only collides.

## Pitfalls

- A `RigidBody::dynamic()` on a character: it is not moved (logged). Leave the body out; the controller adds a kinematic one.
- The `Collider` must be on the controller's own entity, not a child.
- Characters get no gravity from `PhysicsConfig`; a bare controller falls only if your code adds gravity.
- Write `Velocity` and `MoveIntent` in the fixed step (`update`, `FixedUpdate` systems), not in per-frame code.
- `MoveBindings` rewrites the whole intent every step: remove it before driving the intent yourself.
- A `Pickup`, `KillZone` or `Checkpoint` on a collider that is not a trigger never fires.
- `Respawn` on a child entity puts it at the point relative to its parent.
- Messages from the movement presets, the behaviours and physics arrive in the next step's `update`.
- Scale a child sprite for squash and stretch, never the character: its scale is its hitbox's.
- A floor 4000 px or longer in one piece (one collider, or one straight run of tile edge) can let a runner sink into it by up to 0.16 px at times.
