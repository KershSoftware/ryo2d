---
name: ryo-game-dev
description: Building games with Ryo2D - scenes, entities, queries, systems, resources, messages, time and input.
metadata:
  version: 2.0.0
---

# Ryo2D Game Developer

You help users build complete games with Ryo2D: scenes on a stack, entities in one shared ECS world, systems in the engine's phases.

**Complete signatures: the `ryo-api` skill.** Feature skills: `ryo-physics`, `ryo-animation`, `ryo-ui`, `ryo-audio`, `ryo-particles`, `ryo-lighting`, `ryo-messages`, `ryo-config-save`, `ryo-console`, `ryo-controller`, `ryo-procgen`, `ryo-cli`.

## Project Setup

`ryo2d init <name>` (see `ryo-cli`) creates:

```
my-game/
  Cargo.toml   ryo2d dependency, dev profile with optimised dependencies
  Ryo.toml     resolution, vsync, debug flags, release settings
  build.rs     Windows icon embedding
  main.rs      EngineBuilder and the first scene
  assets/      images, sounds, music
```

Larger games put scenes and components under `src/` (`src/scenes/`, `src/components/`) with a `lib.rs`, as `examples/games/*` do.

Keep the generated dev profile: physics and particles are slow with unoptimised dependencies.

```toml
[profile.dev.package."*"]
opt-level = 2
```

Optional crates sit beside the engine in its `crates/` folder:

| Crate | Cargo.toml | Import |
|-------|------------|--------|
| Procedural generation | `ryo2d-procgen = { path = "<ryo2d>/crates/ryo2d-procgen" }` | `use ryo2d_procgen::prelude::*;` |
| Items, inventories, loot | `ryo2d-inventory = { path = "<ryo2d>/crates/ryo2d-inventory" }` | `use ryo2d_inventory::prelude::*;` |

`use ryo2d::prelude::*;` is all a game needs from the engine, including the ECS derives; never add `bevy_ecs` as a dependency.

## Quick Start

```rust
use ryo2d::prelude::*;

#[derive(Component)]
struct Player;

#[derive(Default)]
struct Level {
  player: Option<Entity>,
  score: u32,
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // The camera starts centred on the viewport: world (0, 0) is the screen's top-left
    let (vw, vh) = ctx.camera.viewport_size();

    let sprite = ctx
      .assets
      .create_sprite(Sprite::solid_rect(32, 32, 255, 80, 80, 255), "player");
    let player = ctx
      .world
      .spawn((
        Player,
        Transform::from_xy(vw / 2.0, vh / 2.0),
        Renderable::sprite(sprite).with_z_order(10),
      ))
      .id();
    self.player = Some(player);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }
    let mut dir = Vec2::ZERO;
    if ctx.input.key_down(KeyCode::ArrowLeft) {
      dir.x -= 1.0;
    }
    if ctx.input.key_down(KeyCode::ArrowRight) {
      dir.x += 1.0;
    }
    if let Some(player) = self.player
      && let Some(mut transform) = ctx.world.get_mut::<Transform>(player)
    {
      transform.translation += dir * 200.0 * dt;
    }
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    ui.label(&format!("Score: {}", self.score), 10.0, 10.0, Color::WHITE, None);
    SceneAction::Continue
  }
}

fn main() {
  // Resolution, vsync and fps come from Ryo.toml
  EngineBuilder::new()
    .title("My Game")
    .start_scene(Level::default())
    .build()
    .run()
    .unwrap();
}
```

## The Scene Trait

Every method has a default; implement what the scene needs.

| Method | When it runs |
|--------|--------------|
| `on_enter(ctx)` | The scene becomes active: spawn entities, set the camera, load sounds, add systems |
| `update(ctx, dt) -> SceneAction` | Every fixed step (`dt` is the constant timestep) while the scene is active |
| `ui(ui, ctx) -> SceneAction` | Once per frame, **before** the fixed steps, for every visible scene bottom-up: draw UI, handle UI input |
| `render(gpu)` | Once per frame for visible scenes: GPU-batched extras (particles, parallax) |
| `on_pause(ctx)` / `on_resume(ctx, returned)` | Another scene is pushed on top / the one above popped |
| `on_exit(ctx)` | The scene leaves the stack |
| `is_transparent()` | Scenes below are still drawn and still get `ui` (overlays, HUDs) |
| `updates_below()` | Scenes below still update and run their systems; only the top scene's returned `SceneAction` is applied (a lower scene's is dropped with a warning) |
| `is_physics_paused()` | Pause physics while this scene is on top |
| `cleanup_on_exit()` | Which engine state `Switch`/`Reset` resets (default `SceneCleanup::DEFAULT`) |

Entities are not drawn by `render`: anything with `Transform` + `Renderable` is drawn by the engine every frame.

## SceneContext

| Field | Type | Use |
|-------|------|-----|
| `ctx.world` | `&mut World` | Entities, components, queries, resources |
| `ctx.time` | `&mut Time` | `real`, `game` and `fixed` clocks |
| `ctx.input` | `&mut Input` | Keyboard, mouse, gamepad, text |
| `ctx.camera` | `&mut Camera` | Position, zoom, shake, screen/world conversion |
| `ctx.assets` | `&mut AssetManager` | Sprites (`load_sprite`, `create_sprite`) |
| `ctx.audio` | `&mut Audio` | Sounds and music (calls return `AudioError::Unavailable` without a device) |

Helpers: `add_systems`, `messages::<T>()`, `write_message`, `resource`, `resource_mut`, `resource_or_default`, `insert_resource`, `effects()`, `lighting()`, `tilemaps()`, `render_pipeline()`, `window()`, `cursor()`, `textures()`, `console()`, `spawn_child`, `skip_interpolation`, `screen_size()`, `request_display_mode`, `request_resolution`, `set_cursor*`. The accessors return `Mut<..>`; bind one for several calls: `let mut fx = ctx.effects();`.

**`time`, `input`, `camera` and `assets` are taken out of the world for the length of a callback.** Use the fields; `ctx.world.resource::<Time>()` panics inside a callback. Systems see them as resources (`Res<Time>`) as usual.

## Entities and Components

```rust
#[derive(Component)]
struct Enemy;

let e = ctx
  .world
  .spawn((Transform::from_xy(10.0, 20.0), Renderable::sprite(sprite)))
  .id(); // spawn returns EntityWorldMut; .id() is the Entity

if let Some(mut t) = ctx.world.get_mut::<Transform>(e) {
  t.translation.x += 4.0;
}
ctx.world.entity_mut(e).insert(Velocity::new(50.0, 0.0)); // panics if e is gone
ctx.world.entity_mut(e).remove::<Velocity>();
let is_enemy = ctx.world.get::<Enemy>(e).is_some();
if let Ok(mut entity) = ctx.world.get_entity_mut(e) {
  entity.insert(Enemy); // e may be gone
}
ctx.world.despawn(e); // bool; despawns its children too
```

- `Transform`: `translation: Vec2`, `rotation` (radians), `scale: Vec2`. Build with `from_xy`, `from_translation`, `with_rotation`, `with_rotation_degrees`, `with_scale`, `with_uniform_scale`; `Transform::IDENTITY`.
- `GlobalTransform` is the world-space result, written by the engine; read it for world positions (`global.translation()`), never write it.
- `Velocity { dx, dy }` is integrated by physics for rigid bodies only; anything else moves itself.
- Draw order is `Renderable::with_z_order` (higher in front); `YSort` orders by y within a layer.

## Queries

```rust
// Read
for (transform, velocity) in ctx.world.query::<(&Transform, &Velocity)>().iter(ctx.world) {}

// Write
let mut movers = ctx.world.query::<(&mut Transform, &Velocity)>();
for (mut transform, velocity) in movers.iter_mut(ctx.world) {
  transform.translation += velocity.as_vec2() * dt;
}

// Filter, and collect before changing the world
let enemies: Vec<Entity> = ctx
  .world
  .query_filtered::<Entity, With<Enemy>>()
  .iter(ctx.world)
  .collect();
for enemy in enemies {
  ctx.world.despawn(enemy);
}
```

`Entity`, `Option<&T>`, `With`, `Without`, `Or`, `Has`, `Changed`, `Added` work as query terms. Disabled entities (pooled ones) are skipped unless the query names `Allow<Disabled>`.

## Systems

Scene code is fine for one-off logic; systems suit logic that runs over many entities.

```rust
#[derive(Component)]
struct Spin(f32);

fn spin(time: Res<Time>, mut query: Query<(&mut Transform, &Spin)>) {
  for (mut transform, spin) in &mut query {
    transform.rotation += spin.0 * time.delta(); // the fixed timestep inside FixedUpdate
  }
}

// In on_enter: runs while this scene is active, removed when it leaves
ctx
  .add_systems(Phase::FixedUpdate, spin)
  .expect("scenes may add FixedUpdate systems");
```

| Registration | Phases | Lifetime |
|--------------|--------|----------|
| `ctx.add_systems(phase, systems)` | `Phase::FixedUpdate`, `Phase::Update` (others return `SceneScheduleError`) | While the scene is active (or below an `updates_below` scene) |
| `EngineBuilder::add_systems(phase, systems)` | Any `Phase` | The whole run |

Scene systems run inside `FixedSet::Game` (after the scene's `update`, before physics) and `UpdateSet::Game`. Builder systems go there too, one `add_systems` call at a time, unless the call names a set or another call's system to order against (`.in_set(FixedSet::Juice)`, `.after(FixedSet::Physics)`, `.after(other_system)`); then it keeps exactly that order. Systems may return `Result<(), SystemError>`; an error panics in debug builds and is logged in release. The frame's phases are described in `ryo-engine-dev`.

## Resources

```rust
#[derive(Resource, Default)]
struct Score(u32);

EngineBuilder::new().insert_resource(Score::default()); // or ctx.insert_resource(..)

if let Some(mut score) = ctx.resource_mut::<Score>() {
  score.0 += 1;
}
ctx.resource_or_default::<Score>().0 += 1;
```

Resources outlive scene changes (`SceneCleanup::WORLD` despawns entities, not resources): use them for state shared across scenes. Systems use `Res<Score>` / `ResMut<Score>`.

## Messages

```rust
#[derive(Message, Clone)]
struct CoinCollected {
  value: u32,
}

ctx.write_message(CoinCollected { value: 10 });
for coin in ctx.messages::<CoinCollected>() {
  self.score += coin.value;
}

// In a system; register the type up front with EngineBuilder::add_message::<CoinCollected>()
fn add_score(mut coins: MessageReader<CoinCollected>, mut score: ResMut<Score>) {
  for coin in coins.read() {
    score.0 += coin.value;
  }
}
```

- A message is readable from when it is written until the end of the next frame; each scene and each reader sees it once. Read a type in one place per scene.
- Engine messages: `CollisionStarted`/`CollisionStopped` (physics), `TweenStarted`/`TweenCompleted`, `KnockbackStarted`/`KnockbackFinished`, `SquashStretchStarted`/`SquashStretchFinished`, `AssetReloaded`/`AssetLoadFailed`, `ParticleCollision`. Details: `ryo-messages`.

```rust
for hit in ctx.messages::<CollisionStarted>() {
  if let Some(other) = hit.other(player)
    && ctx.world.get::<Coin>(other).is_some()
  {
    ctx.world.despawn(other);
  }
}
```

## Time

| Clock | What it is | Use |
|-------|------------|-----|
| `ctx.time.real` | Wall clock, never scaled or paused | UI animation, real-time effects |
| `ctx.time.game` | Real delta (capped at 250 ms) times `speed`; zero while paused or in hitstop | `set_speed(0.5)`, `pause()`, `resume()`, `is_paused()`, `hitstop(0.08)`, `elapsed()` |
| `ctx.time.fixed` | Game time cut into constant steps | `timestep()`, `overstep_fraction()`, `step_count()` |

- The step rate is `EngineBuilder::fixed_hz(hz)` (default 60). Slow motion runs fewer steps, pause and hitstop run none; `dt` never changes. Physics follows all of it.
- While the game clock is paused or in hitstop **no `update` runs anywhere**; `ui` still runs every frame, so unpause from `ui`.

## Input

```rust
if ctx.input.key_pressed(KeyCode::Space) {}  // this step only
if ctx.input.key_down(KeyCode::KeyW) {}      // held
if ctx.input.mouse_pressed(MouseButton::Left) {}
let (mx, my) = ctx.input.mouse_position();
let (wx, wy) = ctx.camera.screen_to_world(mx, my);
```

- One-shots (`key_pressed`, `key_released`, `mouse_pressed`, ...) are seen by the first fixed step of a frame; a press in a frame with no step reaches the next step. Handle a press once, in `update` or in a system.
- The UI pass runs before the fixed steps and consumes what it uses: a click on a button never reaches `update`. In `ui`, read input through `ui.input()` and claim keys with `ui.consume_key(key)` / `ui.consume_escape()`.
- A consumed key reads as not pressed and not held for the rest of the frame. `ctx.input.key_pressed_and_consume(key)` reads and consumes in one call.
- Gamepads: `ryo-controller`.

## Scene Stack

| `SceneAction` | Effect |
|---------------|--------|
| `Continue` | Keep running |
| `Push(scene)` | Add on top; the current scene pauses |
| `Pop` | Remove the top scene; the one below resumes |
| `PopReturn(Box<dyn Any>)` | Pop and pass data to `on_resume` (read with `downcast_return::<T>(returned)`) |
| `Switch(scene)` | Replace the top scene (runs cleanup) |
| `Reset(scene)` | Clear the stack, start fresh (runs cleanup) |
| `Quit` | Exit |

`PushWith`, `PopWith`, `PopReturnWith`, `SwitchWith`, `ResetWith` take a `Transition`: `Transition::fade(Duration::from_millis(400))`, `fade_white`, `fade_color(duration, r, g, b)`, `slide(duration, SlideDirection::Left)`.

```rust
return SceneAction::SwitchWith(
  Box::new(Level::default()),
  Transition::fade(Duration::from_millis(400)),
);
```

- Transitions run on real time, once a frame, so a fade finishes even with the game clock paused or slowed.
- A pushed scene that does not `updates_below` stops the scenes below and pauses physics; engine animation and tweens keep running (pause the game clock to freeze them too).
- A pause menu: a transparent pushed scene that handles its buttons in `ui` and returns `SceneAction::Pop`. Scenes under it get no pointer in their `ui`, so its buttons win the clicks.
- Cleanup on `Switch`/`Reset` (not on `Push`/`Pop`): `SceneCleanup` flags `WORLD`, `CAMERA`, `EFFECTS`, `LIGHTING`, `TIME`, `MUSIC`, `MESSAGES`; `DEFAULT` is all but `MUSIC`, plus `NONE` and `ALL`. Combine with `|`, remove with `-`: `SceneCleanup::DEFAULT - SceneCleanup::WORLD` keeps the entities.
- A scene's console commands and systems leave with it.

## Sprites and Anchors

- `Renderable::sprite(handle)` draws the sprite with its **centre** on the entity's position (`Anchor::Center`). Rotation and scale happen about the anchor.
- Things laid out as a grid or from a corner (backgrounds, tiles, panels) take `.with_anchor(Anchor::TopLeft)`; things that move, rotate or scale keep the centre.
- `Renderable::tiled(..)` and composite tiles are laid out from the entity's position (the first tile's top-left), whatever the anchor.
- Sprites: `ctx.assets.load_sprite("player.png")` (relative to `assets/`, returns a `Result`), `ctx.assets.create_sprite(Sprite::solid_rect(w, h, r, g, b, a), "name")`, `Sprite::solid_circle(..)`.

## Physics in Brief

```rust
ctx.world.spawn((
  Transform::from_xy(200.0, 100.0),
  Velocity::zero(),
  Renderable::sprite(sprite),
  RigidBody::new(),
  Collider::box_collider(32.0, 32.0),
));
```

Colliders are centred on the entity's position plus `Collider.offset`, so they line up with a centre-anchored sprite. Bodies must be root entities (no `ChildOf`). Everything else: `ryo-physics`.

## Hierarchy and Interpolation

```rust
let turret = ctx.spawn_child(ship, (Transform::from_xy(0.0, -12.0), Renderable::sprite(sprite)));
ctx.world.detach_keep_world(turret); // leave the parent, stay where it is
```

- A child's `Transform` is relative to its parent; reparent by inserting `ChildOf(parent)`. Despawning a parent despawns its children.
- Inserting a `Transform` places the entity at once (its `GlobalTransform` is right for the rest of the step); later edits through `get_mut` are propagated at the end of the step.
- Smooth motion at a fixed rate below the display rate: add `Interpolated` to moving entities (it covers their children). After a teleport call `ctx.skip_interpolation(entity)` so it does not slide.

## Pooling

```rust
#[derive(Component, Clone)]
struct Bullet;

let mut bullets = Pool::new(
  move || (Bullet, Transform::IDENTITY, Velocity::zero(), Renderable::sprite(sprite.clone())),
  PoolConfig::default(),
);

if let Some(bullet) = bullets.acquire(ctx.world) {
  // acquire does not reset components: set everything you rely on
  ctx
    .world
    .entity_mut(bullet)
    .insert((Transform::from_translation(at), Velocity::new(0.0, -400.0)));
}

// When a bullet is done (off screen, hit something)
bullets.release(ctx.world, spent);
```

Released entities carry `Disabled`: every query, physics and rendering skip them. An acquired entity does not slide from where it was released. `PoolConfig` fields: `initial_capacity`, `max_capacity` (0 = unlimited), `expand_batch_size`, `exhausted_policy` (`Expand`, `ReturnNone`, `RecycleOldest`).

## Camera and Coordinates

- The camera's position is the centre of the view. It starts (and `SceneCleanup::CAMERA` resets it) at half the viewport, so world (0, 0) is the screen's top-left and world coordinates equal screen pixels until the camera moves. +Y is down.
- `set_position`, `follow(x, y, lerp)`, `snap_to`, `set_zoom`, `add_trauma(0.4)` (shake, in real time), `screen_to_world`, `world_to_screen`, `visible_bounds()`.
- Clamp a centred sprite with its half size: `t.translation.x.clamp(half_w, screen_w - half_w)`.

## Resolution and Window

Resolution comes from `Ryo.toml`; the game always works in its design resolution and the window scales it.

| `WindowMode` | Behaviour |
|--------------|-----------|
| `Fixed` | No resizing |
| `ResizableAspectRatio` | Scales with letterboxing (default) |
| `ResizableIntegerScale` | Pixel-perfect integer scaling |
| `ResizableStretch` | Fills the window, may distort |

`DisplayMode` (`Windowed`, `ExclusiveFullscreen`, `BorderlessFullscreen`) is how the window appears: `ctx.request_display_mode(DisplayMode::BorderlessFullscreen)`. Builder overrides (`resolution`, `target_fps`, `vsync`, `fixed_hz`, `window_mode`) beat `Ryo.toml`; saved player preferences beat both when `with_user_preferences` includes them (`ryo-config-save`).

## Navigation

Nothing in the engine moves navigation agents: the scene owns a `NavigationGrid` and a `Pathfinder` and moves entities along the paths.

```rust
let grid = NavigationBuilder::new(800.0, 600.0)
  .with_cell_size(16.0)
  .build_from_world(ctx.world); // blocks every non-trigger Collider
```

`NavAgent::new(speed).with_smoothing(PathSmoothing::LineOfSight)`, `NavTarget::position(x, y)` / `NavTarget::follow(entity)`, and `steering::{separation, seek, flee, arrive, cohesion, alignment}`. See `src/core/navigation/AGENTS.md` and `ryo-api`.

## Visual Tests

`ryo2d::testing::HeadlessEngine` runs a scene without a window, one fixed step and one render per `step`, and returns frames as pixels:

```rust
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

#[test]
fn player_is_drawn_at_spawn() {
  let Some(mut engine) = HeadlessEngine::new_or_skip(
    EngineBuilder::new().resolution(320, 180).start_scene(Level::default()),
  ) else {
    return; // no GPU
  };

  engine.step_frames(5).unwrap();
  engine.tap_key(KeyCode::Space); // press_key, release_key, press_mouse, mouse_move, type_text
  engine.step_frames(10).unwrap();

  let frame = engine.frame().unwrap(); // RgbaImage at the game resolution
  assert_eq!(frame.get_pixel(160, 90).0, [255, 0, 0, 255]);
}
```

- Same scene, steps and input give identical frames: seed your RNG and use `ParticleSystem::with_seed(n)`.
- Headless runs have no audio device, gamepads or user preferences; `Ryo.toml` is honoured. `engine.world()` / `world_mut()` reach the world between steps.

## Pitfalls

- Spawning returns `EntityWorldMut`: store `.id()`, not the spawn result.
- Positions are `t.translation.x`, not `t.x`; rotation is radians.
- A sprite placed by its top-left corner needs `.with_anchor(Anchor::TopLeft)`; otherwise it is centred on its position.
- Do not change the world while iterating a query over it: collect entities first.
- Keys the UI acts on: read and consume them in `ui`, or `update` sees them too.
- No `update` runs while the game clock is paused.
- Integrate your own `Velocity` for entities without a `RigidBody`.
