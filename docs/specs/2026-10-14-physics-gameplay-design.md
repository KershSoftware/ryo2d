# Physics Gameplay (3b): Characters, One-Way Platforms, Triggers, Tiles - Design

## Overview

Sub-project 3b of the professional-grade overhaul builds gameplay physics on 3a's simulation (`docs/specs/2026-10-13-physics-simulation-design.md`, Rapier 0.36). It adds a **character controller** on Rapier's `KinematicCharacterController`, **platformer and top-down movement presets** driven by a **movement intent**, **one-way platforms** for bodies and characters, **trigger messages and contents** with ready-made **Pickup, KillZone, Checkpoint and Respawn** behaviours, **tile colliders** merged into seamless outlines, and **knockback that goes through physics**. The bundled games move onto these and lose their hand-written collision maths.

**Why:** every character in the bundled games is a platformer that does its own gravity, jumping, landing and wall checks with hand AABB maths (bullet-hell's player, leap-king's bodiless player, the networking demo). Bullet hits, pickups, checkpoints and kill zones are hand distance tests; only breakout uses a real trigger. Knockback moves non-dynamic entities by `Transform`, straight through walls. Nothing builds physics colliders from tiles. A game system ordered with `.after(other)` can be scheduled inside the physics chain, where the write-back overwrites its movement.

**Owner decisions (2026-10-14):**
- **Controller:** a move-and-slide core plus a platformer preset (run, acceleration, air control, jump height, max fall speed) with **every extra**: jump forgiveness (coyote time, jump buffering, jump cut), air jumps, wall slide and wall jump, dash. **Plus a top-down preset.**
- **Approach:** wrap Rapier's `KinematicCharacterController` for the sweep (slide, slopes, step, snap, pushing) and add our own layer for floor/wall/ceiling, moving platforms, one-way and knockback. Presets read a `MoveIntent`, not the keyboard.
- **Tiles:** a grid API plus an adapter for today's Tiled `Tilemap`; the later tilemap rewrite calls the same grid API.
- **Triggers:** enter/exit messages, live contents, and ready-made behaviours (`Pickup`, `KillZone`, `Checkpoint`, `Respawn`).
- **Follow-ups folded in:** keep game systems out of the physics step; clamp the engine's schedule ticks.
- **Breaking changes:** acceptable; networking stays out of scope beyond keeping leap-king and the networking demo working.

**Out of scope:** crush detection; incremental tile rebuilds; slopes other than 45° and per-cell custom shapes; ladders, swimming, climbing; a dynamic-body (force-driven) character; replicating the new components in the networking layer (games replicate what they already do); the tilemap rewrite itself.

## Architecture

**Modules:**

| Path | Holds | Rapier? |
|---|---|---|
| `src/core/physics/character.rs` | `CharacterController`, `CharacterState`, `CharacterHit`, the `move_characters` system | yes |
| `src/core/physics/one_way.rs` | the physics hooks (one-way contacts) and the controller's one-way rule | yes |
| `src/core/physics/triggers.rs` | `TriggerEntered`, `TriggerExited`, `TriggerContents` | yes |
| `src/core/physics/tiles.rs` | `TileColliders`, `TileCell`, `SlopeCorner`, outline merging, the rebuild system | yes |
| `src/core/gameplay/intent.rs` | `MoveIntent`, `MoveBindings`, the per-frame input latch | no |
| `src/core/gameplay/platformer.rs` | `PlatformerMovement`, `PlatformerState` | no |
| `src/core/gameplay/top_down.rs` | `TopDownMovement`, `TopDownState` | no |
| `src/core/gameplay/events.rs` | `MovementEvent`, `MovementEventKind`, `PickedUp`, `Killed`, `CheckpointReached` | no |
| `src/core/gameplay/zones.rs` | `Pickup`, `KillZone`, `Checkpoint`, `Respawn` | no |
| `src/core/gameplay/knockback.rs` | `Knockback` and its system (moved from `ecs/knockback.rs` and `systems/knockback.rs`) | no |

`gameplay` depends on `physics`, never the reverse; rapier2d stays inside `src/core/physics/`. The Tiled adapter lives in the tilemap module (`Tilemap::tile_colliders`), so tilemap depends on physics. Everything public is in the prelude.

### One fixed step

1. **Snapshot**, **Juice** (tweens, squash; knockback leaves this set).
2. **Scene**, **Game**: the game writes `MoveIntent` (or an AI does), or a character's `Velocity` for custom movement.
3. **Movement** (new `FixedSet::Movement`): the zone behaviours act on the last step's trigger messages (pickups, kill zones and respawns, checkpoints); tile colliders whose grid changed are rebuilt; knockback advances; the presets turn intent and `CharacterState` into `Velocity` and clear the intent's pressed flags.
4. **Physics**: one exclusive system that runs a private schedule holding the chain: `forget_removed`, `track_disabled`, `propagate_transforms`, `register_bodies`, `sync_colliders`, `push_changes`, **`move_characters`**, `sync_joints`, `step_simulation` (now with the one-way hooks), `read_back`, `emit_messages` (now also the trigger messages and `TriggerContents`). No game system can be ordered into the middle of it.
5. **Transforms**, **Trails**.

**Input latch:** `MoveBindings` reads input once per frame, before the fixed steps, into `MoveIntent`. `jump_pressed` and `dash_pressed` stay set until a fixed step's presets consume them, so a tap on a frame that runs no fixed step is not lost and one press is not used twice in a frame that runs two.

**Change ticks:** every 1,024 frames `clear_trackers_system` also calls `check_change_ticks` on the engine's schedules, the scenes' schedules and the private physics schedule, so a system idle for a long time never misreads change ticks.

**Builder systems:** `EngineBuilder::add_systems` keeps its rules (no set named: `FixedSet::Game`); because the physics step is one system, `.after(other)` can no longer place a system inside it.

## The character controller

**Setup:** add `CharacterController` to an entity with a `Collider`. The engine inserts `RigidBody::kinematic().lock_rotation()` when the entity has no body; a dynamic or fixed body is warned about once and the entity is not moved. The collider on the character's own entity is the swept shape (child colliders follow the body as usual). The character collides with what its collider's layer and mask allow; triggers never block it.

```rust
#[non_exhaustive]
pub struct CharacterController {
  pub drop_through: bool, // set by the game; cleared by the engine once below the platform
  /* mode, max_slope, step_height, snap_to_ground, skin, push: private, set by builders */
}
CharacterController::platformer()        // up = -Y: floors, walls, ceilings
CharacterController::top_down()          // no up: every surface is a wall, no snap or step
  .with_max_slope(degrees)               // default 45: steeper surfaces are walls; the character slides down them
  .with_step_height(px)                  // default off: walk up ledges this high
  .with_snap_to_ground(px)               // default: a fifth of the collider's height (Rapier's default); 0 disables
  .with_skin(px)                         // default: Rapier's 1% of the collider's height
  .with_push(bool)                       // default true: push dynamic bodies, mass from the collider's density
```

**Inputs:** `Velocity` is what the game wants this step (its own gravity, run and jump, or a preset's). Writing `Transform` teleports the character with no sweep (respawns, network corrections), as for any kinematic body. `Knockback` adds its own term (below).

**`move_characters`, each step:**
1. The desired translation is `(Velocity + knockback velocity) × dt` plus the floor body's motion this step (its next pose minus its pose, at the character's contact point, so a rotating platform carries too).
2. Rapier's `move_shape` sweeps it with a query filter excluding the character's own body and triggers, applying its layer/mask and the one-way rule.
3. The result is set as the body's next kinematic position, so contacts, triggers and the write-back work as for any kinematic body.
4. Each hit is classified against `up`: within `max_slope` of up is floor, within `max_slope` of down is ceiling, else wall (top-down: always wall).
5. `Velocity` becomes the real velocity: the component into each floor, wall or ceiling hit is removed (landing zeroes the fall, a head bump stops the rise, a wall stops the run). Leaving a moving floor adds its velocity to `Velocity`, so a jump off a platform keeps its momentum.
6. With `push`, Rapier's `solve_character_collision_impulses` pushes the dynamic bodies hit.

**Output** (written by the engine every step; games read it):

```rust
#[non_exhaustive]
pub struct CharacterState {
  pub on_floor: bool, pub on_wall: bool, pub on_ceiling: bool,
  pub floor_normal: Vec2, pub wall_normal: Vec2,      // zero when none
  pub floor: Option<Entity>, pub platform_velocity: Vec2,
  pub landed: bool, pub left_floor: bool,             // true on the step it happens
  pub sliding_down_slope: bool,
  pub velocity_before: Vec2,                          // the Velocity the step asked for, before clipping (for bounces)
  pub hits: Vec<CharacterHit>,                        // everything the move touched this step
}
pub struct CharacterHit { pub entity: Entity, pub point: Vec2, pub normal: Vec2 }
```

**Limits:** a solid moving into a character pushes it out on the next step (Rapier's depenetration); crushing between two solids is not modelled and leaves the character overlapping. Queries inside the sweep see colliders at their last stepped pose (game teleports of other colliders apply at this step's simulation).

## One-way platforms

- `Collider::one_way()`: solid only against things arriving from above (the side its local `-Y` faces); `Collider::one_way_towards(direction)`: solid against things arriving from `direction`'s side. The direction turns with the collider's rotation. Works on any shape; only marked colliders run the hook.
- **Bodies:** marked colliders get Rapier's `ActiveHooks::MODIFY_SOLVER_CONTACTS`; the step's hooks call `update_as_oneway_platform(direction, 45°)` (flipped when the platform is the pair's second collider). A crate tossed up through a platform lands on top; a body part-way through is not snapped back (Rapier tracks each crossing in the manifold).
- **Characters:** the sweep ignores a one-way collider unless the character's shape was entirely on its solid side at the start of the step and the move goes into it. Jumping up through, walking off and side entry all pass.
- **Drop-through:** with `drop_through` set, the character ignores the one-way colliders it stands on until it is entirely past them; the engine then clears the flag. The platformer preset sets it on down + jump.

## Movement presets

**`MoveIntent`** (component; the one input every preset reads):

```rust
pub struct MoveIntent { pub direction: Vec2, pub jump_held: bool, pub jump_pressed: bool, pub dash_pressed: bool }
```

`direction` is -1..1 per axis (analog values from sticks; +Y down). Written by `MoveBindings`, game code, an AI or networking.

**`MoveBindings`** (optional component): fills `MoveIntent` from input once per frame. Defaults: arrows/WASD move, Space jump, Shift dash; gamepad stick or d-pad move, South jump, West dash, any gamepad. Builders: `with_move_keys(..)`, `with_jump_keys(..)`, `with_dash_keys(..)`, `with_gamepad(id)`, `keyboard_only()`.

**`PlatformerMovement`** (needs `CharacterController::platformer()`; warned once otherwise). Writes `Velocity` from the intent and `CharacterState`. Starting defaults (tuned in the demo):

| Option | Builder | Default |
|---|---|---|
| Run speed | `with_run(speed, accel_time, decel_time)` | 180 px/s, 0.08 s to full speed, 0.06 s to stop |
| Air control | `with_air_control(fraction)` | 0.65 of ground acceleration |
| Jump | `with_jump(height_px, time_to_apex_s)` | 64 px, 0.38 s (gravity and launch speed derived) |
| Fall | `with_fall(gravity_multiplier, max_speed)` | 1.5×, 600 px/s |
| Coyote time | `with_coyote_time(s)` | 0.1 s |
| Jump buffer | `with_jump_buffer(s)` | 0.1 s |
| Jump cut | `with_jump_cut(factor)` | 0.5 (upward speed × factor on early release; 1 disables) |
| Air jumps | `with_air_jumps(n)` | 0 (refilled on landing and on wall jump) |
| Wall slide | `with_wall_slide(max_speed)` | off |
| Wall jump | `with_wall_jump(launch: Vec2, lock_time)` | off (launch x points away from the wall; input ignored for `lock_time`) |
| Dash | `with_dash(speed, duration, cooldown)` | off (8-way from input, facing when none; no gravity while dashing; one air dash, refilled on landing) |
| Knockback control | `with_knockback_control(fraction)` | 0 (no steering while knockback is active) |

The preset uses its own gravity, not `PhysicsConfig`'s (characters are kinematic). Down + jump on a one-way floor sets `drop_through` instead of jumping.

**`TopDownMovement`** (needs `CharacterController::top_down()`): `with_speed(max, accel_time, decel_time)` (default 160 px/s, 0.08 s, 0.1 s), normalised diagonals, analog magnitude kept; optional `with_dash(speed, duration, cooldown)`; `with_knockback_control(fraction)`.

**State and messages:** `PlatformerState { facing, air_jumps_left, dashing, dash_ready, wall_sliding, .. }` and `TopDownState { facing, dashing, dash_ready, .. }` (engine-written, `#[non_exhaustive]`). `MovementEvent { entity, kind }` with `MovementEventKind::{Jumped, AirJumped, WallJumped, Dashed, Landed}` lets games hang sound, particles and squash on movement. The presets are plain functions of intent, state and `CharacterState`, so client prediction can re-run them.

## Triggers

- **Messages:** trigger overlaps leave `CollisionStarted`/`CollisionStopped` (their `trigger` field goes; those messages now mean solid contact only) and become `TriggerEntered { trigger, other, other_body }` / `TriggerExited { .. }`, where `trigger` is the trigger collider's entity, `other` the other collider's entity and `other_body` its body (or `None` for a bare collider). Two triggers overlapping produce one message per trigger.
- **Once per body:** a body with several colliders inside one trigger enters on its first collider and exits when its last one leaves (fixes 3a's duplicate-trigger minor).
- **`TriggerContents`** (engine-added to every trigger collider, updated after each step): the bodies, or bare colliders, inside now, in entry order (`entities()`).
- What can set a trigger off stays the existing layer/mask test.

### Behaviours

Each goes on a trigger collider (warned once if the collider is not a trigger) and acts in `FixedSet::Movement` on the last step's `TriggerEntered` messages.

- **`Pickup`:** the first entity the mask allows to enter collects it: `PickedUp { pickup, by }` is written and the pickup despawned. `Pickup::new().respawn_after(seconds)` disables it instead (3a's `Disabled`) and re-enables it after the delay.
- **`Respawn { point }`** on actors: `Respawn::here()` takes the actor's position when the component is added; `Respawn::at(point)`.
- **`KillZone`:** writes `Killed { entity, zone }`. An entity with `Respawn` is teleported to its point with `Velocity`, `Knockback` and preset state cleared. Others get only the message, unless the zone is `KillZone::new().despawning()` (bullets, enemies falling out of the world). Nothing is despawned silently by default.
- **`Checkpoint`:** an entity with `Respawn` that enters gets its point moved to the checkpoint's position (plus `with_offset(Vec2)`); `CheckpointReached { checkpoint, entity }` is written only when the point changes.

## Tile colliders

```rust
pub enum TileCell { Empty, Solid, OneWay, Slope(SlopeCorner) }
pub enum SlopeCorner { BottomLeft, BottomRight, TopLeft, TopRight } // the filled right-angle corner of a 45° half-cell
TileColliders::new(width, height, tile_size: Vec2)
TileColliders::from_cells(width, height, tile_size, cells: Vec<TileCell>) // row-major, row 0 at the top
  .set(x, y, cell) / .get(x, y)
  .with_layer(..) / .with_mask(..) / .with_friction(..) / .with_restitution(..)
  .solid_rects()                                  // greedy merged rectangles instead of outlines
```

- **Component:** `TileColliders` lives on an entity whose `Transform` places cell (0, 0)'s top-left; +Y down. Its colliders are child entities. With no body they are fixed; with a kinematic body the whole map moves. A changed grid (any `set`) is rebuilt whole in `FixedSet::Movement`.
- **Outlines (default):** each solid or slope cell contributes its polygon's edges, wound consistently; edges shared by neighbouring cells cancel; the rest are chained into closed loops (holes included) and collinear runs are merged. Each loop is one `Collider::polyline`. A floor is one edge with no seams, so characters and crates do not snag at tile joins (Rapier 2D cannot fix polyline internal edges itself), and slopes meet floors seamlessly.
- **One-way cells:** each horizontal run of one-way cells becomes one `Collider::segment(..).one_way()` along its top edge.
- **Hollow outlines:** a body that ends up inside a solid region is not pushed out; `.solid_rects()` trades seams for solid interiors (one-way and slope cells still produce their own colliders).
- **Tilemap adapter:** `Tilemap::tile_colliders(&self) -> TileColliders` reads each tile's `solid` and `platform` flags (OR across layers) and a `slope` custom property (`"bottom-left"`, `"bottom-right"`, `"top-left"`, `"top-right"`).
- **Navigation:** `NavGrid::block_tiles(&TileColliders, origin: Vec2)` blocks exactly the solid and slope cells (`block_collider` would block a whole outline's bounding box).

## Knockback

`Knockback` keeps its API (direction, force in px/s, decay, accumulate, `KnockbackStarted`/`KnockbackFinished`) and moves to `gameplay/knockback.rs`, running in `FixedSet::Movement`. Its effect depends on the target:

- **Character** (`CharacterController`): its velocity is a separate term in the sweep, not part of `Velocity`, so it decays on its own curve and the presets do not fight it. The part of it running into a hit surface is removed, so a player knocked into a wall stops there. The presets ignore steering while it is active unless `with_knockback_control(fraction)`; their gravity keeps running.
- **Dynamic body:** one impulse at the start, `mass × direction × force`, so any mass leaves at the same speed; physics slows it afterwards and the decay rate does not apply; the started/finished messages keep the knockback's own timing.
- **Kinematic body without a controller, fixed body:** ignored, warned once.
- **No body:** `Transform` is moved as today (decorative juice).

## Public API changes

- Added: `CharacterController`, `CharacterState`, `CharacterHit`; `Collider::one_way`, `one_way_towards`; `TriggerEntered`, `TriggerExited`, `TriggerContents`; `TileColliders`, `TileCell`, `SlopeCorner`; `MoveIntent`, `MoveBindings`, `PlatformerMovement`, `PlatformerState`, `TopDownMovement`, `TopDownState`, `MovementEvent`, `MovementEventKind`; `Pickup`, `PickedUp`, `KillZone`, `Killed`, `Checkpoint`, `CheckpointReached`, `Respawn`; `Tilemap::tile_colliders`; `NavGrid::block_tiles`; `FixedSet::Movement`.
- Changed: `CollisionStarted`/`CollisionStopped` lose `trigger` and no longer report trigger overlaps; `Knockback` acts through physics per target; `Knockback` moves module (prelude path unchanged).
- Internal: the physics chain runs as one exclusive system over a private schedule.

## Migration (in place, every step green)

1. The physics step as one system over a private schedule; `FixedSet::Movement`; schedule tick clamping.
2. Trigger messages split out, `TriggerContents`, once-per-body; breakout and the `physics` demo ported.
3. One-way colliders for bodies (the hooks).
4. The controller core: `move_characters`, modes, classification, `Velocity` update, platform carry, pushing, `CharacterState`.
5. One-way for characters and drop-through.
6. Knockback through physics (moved to `gameplay`); the `knockback` demo updated.
7. `MoveIntent`, `MoveBindings` and the input latch.
8. `PlatformerMovement`: run, air control, jump, fall, forgiveness; `MovementEvent`.
9. Platformer extras: air jumps, wall slide and wall jump, dash.
10. `TopDownMovement`.
11. Behaviours: `Pickup`, `Respawn`, `KillZone`, `Checkpoint`.
12. `TileColliders`: outlines, slopes, one-way runs, `solid_rects`, rebuild; `Tilemap::tile_colliders`; `NavGrid::block_tiles`.
13. Games: bullet-hell; leap-king and the networking demo; flappy.
14. The `character` demo.
15. Documentation and the benchmark.

## Games and demos

- **bullet-hell:** the player gets `CharacterController::platformer()`, `PlatformerMovement` with wall slide and wall jump, and `MoveBindings`; `PlayerVelocity` and the hand AABB code go. Platforms become `one_way()`; the moving barriers carry riders through the controller. Bullet hits use `TriggerEntered`. Squash and stretch hang off `MovementEvent`.
- **leap-king:** the local (and host-simulated) players get `CharacterController::platformer()`; the charge jump writes `Velocity`; the wall bounce reflects `CharacterState::velocity_before` off `wall_normal`. Platforms become one-way; moving platforms carry. The player-against-player push becomes knockback; the head bounce reads `CharacterState::floor`. Checkpoint platforms get `Checkpoint` triggers and players get `Respawn`; the fall-death rule and respawn delay stay game code. Remote players keep no controller and stay `Transform`-driven, so dead reckoning and server corrections work as now.
- **networking demo:** the same treatment as leap-king.
- **breakout:** the death zone reads `TriggerEntered`.
- **flappy:** scoring moves from a hand x comparison to trigger gates between the pipes.
- **knockback demo:** a character page (stopped by walls) and a dynamic page (impulses by mass).
- **state-machine, squash-stretch, blob-shadows demos:** unchanged (they demonstrate other features).
- **New `character` demo** (demo-kit standard, no files, runs on the web):
  - *Platformer*: a tile level with slopes and one-way rows, every preset extra switchable live in the settings panel, moving platforms, crates to push, coins (`Pickup`), spikes (`KillZone`), flags (`Checkpoint`).
  - *Top-down*: `TopDownMovement` with dash, tile walls, crates and pickups.
  - *Custom*: the core controller with hand-written movement and a live `CharacterState` readout (floor/wall/ceiling, normals, platform velocity).

## Testing

- **Unit (physics):** a character falls and lands (`on_floor`, `landed` once, `Velocity.dy` zeroed); walks up a 30° slope and slides down a 60° one; stops at a wall (`on_wall`, `wall_normal`, `velocity_before` kept); bumps a ceiling; steps up a ledge below `step_height` and not above; snaps down a slope; rides a horizontally and a vertically moving platform and a rotating one; keeps a platform's momentum on leaving it; pushes a dynamic crate (and not with `with_push(false)`); teleports by `Transform`; top-down slides along walls and is never `on_floor`; a dynamic body is warned about and not moved. One-way: a crate tossed up through lands on top; a character jumps through and lands; side entry passes; drop-through falls through and clears the flag; a rotated one-way platform blocks along its own up. Triggers: entered and exited once per body with two colliders; trigger-trigger produces two messages; `TriggerContents` order; `CollisionStarted` no longer reports triggers. Tiles: loop counts (a square, an L, a ring with a hole gives two loops), a box sliding across a 100-tile floor never stops and a character running across never snags, slopes merge into the floor loop, one-way runs, `set` rebuilds, `solid_rects`, the Tiled adapter, `NavGrid::block_tiles`. Knockback: a character knocked into a wall stops at it; dynamic bodies of different mass leave at the same speed; a fixed body is ignored and warned once; a bodiless entity moves by `Transform`.
- **Unit (gameplay):** the platformer reaches its jump height within 1 px at its apex time within one step; coyote time allows a jump within its window and not after; a buffered press jumps on landing; jump cut; air jumps count down and refill; wall slide caps fall speed; wall jump launches away and locks input; dash speed, duration, cooldown, no gravity, one air dash; knockback control 0 ignores steering; top-down acceleration and normalised diagonals; `MoveBindings` latching (a press on a frame with no fixed step reaches the next step; one press jumps once over a frame with two steps). Behaviours: a pickup is collected once (and respawns with `respawn_after`); a kill zone respawns an entity with `Respawn` and clears its velocity, only writes `Killed` otherwise, despawns with `despawning`; a checkpoint moves the point and writes its message only on change.
- **Schedule:** a system added with `.after(other)` never runs inside the physics step; the tick clamp reaches the engine's, the scenes' and the physics schedules.
- **E2E (`HeadlessEngine`):** the `character` demo joins the tour; bullet-hell and leap-king run headless with scripted input and land on platforms.
- **Benchmark:** `benches/physics.rs` gains 100 characters running on a tile level; time per step recorded.
- **Gates:** fmt, workspace clippy and wasm32 clippy clean; the demo tour with every changed page checked by eye; the games captured and checked by eye; the `character` demo on the web in Chromium, WebKit and Firefox.

## Documentation

- Skills: a new `ryo-character` (controller, presets, one-way, triggers and behaviours, tiles, recipes: a platformer player, a top-down player, an AI enemy filling `MoveIntent`, custom movement on the core); `ryo-physics` (one-way, trigger messages, knockback); `ryo-api` (every signature above); `ryo-game-dev` (recipes moved onto the presets); copies in `src/cli/skills/`.
- AGENTS.md: `src/core/physics/AGENTS.md` (the step as one system, `move_characters`, hooks, triggers, tiles); a new `src/core/gameplay/AGENTS.md` (presets, intent latch, behaviours, knockback); `src/core/AGENTS.md` (the `Movement` set); `docs/architecture/overview.md`.

## Design decisions

- **Wrap Rapier's controller:** its sweep (slide, slopes, step, snap, push impulses) is tested code; our layer adds what games need from Godot's model (floor/wall/ceiling, `Velocity` as the real velocity, platform carry). The sweep sits behind `CharacterController`, so our own collide-and-slide could replace it without changing games.
- **`Velocity` in, real `Velocity` out:** Godot's `move_and_slide` contract, widely known; games and presets never accumulate gravity into a floor.
- **Presets read `MoveIntent`:** one preset drives players, AI enemies and networked remotes alike; input binding is a separate, optional component.
- **Preset gravity is the preset's own:** platformer feel is set by jump height and time to apex, not by world gravity.
- **Knockback as its own term:** it decays on its curve and is clipped by walls without the presets overwriting it.
- **Triggers separate from collisions:** collision messages mean solid contact; trigger code reads trigger messages; no field to check.
- **Behaviours act a step later, in `Movement`:** they read the previous step's messages, keep game rules out of the physics schedule, and their `Transform` writes (respawns) teleport at the next `push_changes`.
- **Outline tile colliders by default:** seamless floors matter more to platformers than solid interiors; `solid_rects` covers the rest.
- **The physics step as one system:** the chain's ordering and tick invariants can no longer be broken by how a game orders its systems.

## Risks

- Rapier's `KinematicCharacterController` has known rough edges on slopes and ceilings; the controller tests pin the behaviour, and a failure there is fixed in our layer or by replacing the sweep behind the same API.
- The one-way rule for characters on rotated platforms and at platform ends needs its own tests.
- Outline colliders are hollow; documented, with `solid_rects` as the alternative.
- leap-king's networked prediction now mixes controller-moved local players with `Transform`-driven remotes; checked headless and by eye by the owner.
- The controller's per-character cost (one sweep per character per step) is measured by the benchmark.
