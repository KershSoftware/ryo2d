---
name: ryo-game-dev
description: Building games with Ryo2D - scenes, entities, queries, systems, resources, messages, time and input.
metadata:
  version: 2.11.0
---

# Ryo2D Game Developer

You help users build complete games with Ryo2D: scenes on a stack, entities in one shared ECS world, systems in the engine's phases.

**Complete signatures: the `ryo-api` skill.** Feature skills: `ryo-physics`, `ryo-character` (players, enemies, movement presets, pickups, kill zones, checkpoints, tile levels), `ryo-assets` (loading images, fonts, shaders, UI files, themes and audio; handles and hot reload), `ryo-animation`, `ryo-ui`, `ryo-text` (fonts, rich text, world text), `ryo-audio` (sound definitions, the mixer, volume settings, spatial sound, music), `ryo-tilemap` (levels from Tiled and LDtk: layers, objects, collision, editing, worlds), `ryo-particles`, `ryo-lighting`, `ryo-messages`, `ryo-config-save`, `ryo-console`, `ryo-controller`, `ryo-procgen`, `ryo-cli`.

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
    // The main camera starts centred on the design area (the resolution):
    // world (0, 0) is its top-left
    let center = ctx.camera().position();

    // An image made in code; from a file: ctx.assets().load_now::<Image>("player.png")
    let sprite = ctx.assets().add(Image::solid_rect(32, 32, 255, 80, 80, 255));
    let player = ctx
      .world
      .spawn((
        Player,
        Transform::from_xy(center.x, center.y),
        Renderable::image(sprite).with_z_order(10),
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
    // The default style: Inter, 14 px. `ui.text(..)` and `Text2d` style it further (see `ryo-text`)
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
| `render(gpu)` | Once per frame for visible scenes: GPU-batched extras (many-sprite effects) |
| `on_pause(ctx)` / `on_resume(ctx, returned)` | Another scene is pushed on top / the one above popped |
| `on_exit(ctx)` | The scene leaves the stack |
| `is_transparent()` | Scenes below are still drawn and still get `ui` (overlays, HUDs), but only the top scene's widgets take input |
| `updates_below()` | Scenes below still update and run their systems; only the top scene's returned `SceneAction` is applied (a lower scene's is dropped with a warning) |
| `is_physics_paused()` | Pause physics while this scene is on top |
| `holds_audio_below()` | Default `true`: while this scene is on top, the scenes it does not update are not heard (their voices on `sfx`, `voice` and `ambience` are held, and resume when they update again); `false` leaves them audible |
| `cleanup_on_exit()` | Which engine state `Switch`/`Reset` resets (default `SceneCleanup::DEFAULT`) |

Entities are not drawn by `render`: anything with `Transform` + `Renderable` is drawn by the engine every frame.

## SceneContext

| Field | Type | Use |
|-------|------|-----|
| `ctx.world` | `&mut World` | Entities, components, queries, resources |
| `ctx.time` | `&mut Time` | `real`, `game` and `fixed` clocks |
| `ctx.input` | `&mut Input` | Keyboard, mouse, gamepad, text |

Helpers: `assets()` (the asset registry: `load_now`, `load`, `add`, `get`; `ryo-assets`), `audio()` (play sounds and music, snapshots; `ryo-audio`), `saves()` (save, load, list and delete game saves; `ryo-config-save`), `settings()` (the player's settings: volumes, window size, display mode, vsync, the FPS counter and the game's own sections, applied at once and saved by the engine; `ryo-config-save`), `maps()` (spawned tile maps: read and edit tiles, Wang terrain, cells and world positions, world levels; `ryo-tilemap`), `camera()` (the main camera: position, zoom, rotation, shake, screen/world conversion), `actions()` (bind and rebind input actions), `ui_theme()` (the UI theme), `follow_ui_theme(handle)` (follow a `.theme.ron` asset), `render_targets()`, `add_systems`, `messages::<T>()`, `write_message`, `resource`, `resource_mut`, `resource_or_default`, `insert_resource`, `effects()`, `lighting()`, `look()`, `render_pipeline()`, `window()`, `cursor()`, `console()`, `spawn_child`, `skip_interpolation`, `screen_size()`, `request_ui_scale`, `set_cursor*`. The accessors return `Mut<..>`; bind one for several calls: `let mut fx = ctx.effects();`.

**`time` and `input` are taken out of the world for the length of a callback.** Use the fields; `ctx.world.resource::<Time>()` panics inside a callback. Systems see them as resources (`Res<Time>`) as usual.

## Entities and Components

<!-- given: sprite: Handle<Image> -->
```rust
#[derive(Component)]
struct Enemy;

let e = ctx
  .world
  .spawn((Transform::from_xy(10.0, 20.0), Renderable::image(sprite)))
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
- `Velocity { dx, dy }` is integrated by physics for rigid bodies, and a character (`CharacterController`) is swept by it; anything else moves itself.
- Draw order is `Renderable::with_z_order` (higher in front); `YSort` orders by y within a layer. Ties (equal z, or equal sort y) draw in entity index order, the higher index in front. That is spawn order only until something is despawned: bevy gives its index to a later spawn, which then draws behind the entities spawned before it. To keep one sprite in front of another (a hit flash, a popup), give it a higher `z_order`. A query's own order is neither and changes whenever a resource or a new component combination is added: where it shows (lines drawn from a query, say), sort by `entity.index_u32()` first.

## Queries

<!-- with: #[derive(Component)] struct Enemy; -->
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

Scene systems run inside `FixedSet::Game` (after the scene's `update`, before physics) and `UpdateSet::Game`, and nowhere else. Builder systems go there too, one `add_systems` call at a time, unless the call names a set or another call's system to order against (`.in_set(FixedSet::Juice)`, `.after(FixedSet::Physics)`, `.after(other_system)`); then it keeps exactly that order. Systems may return `Result<(), SystemError>`; an error panics in debug builds and is logged in release. The frame's phases are described in `ryo-engine-dev`.

`ctx.add_systems` refuses a call, and adds none of its systems, in two cases:

- **It names a place scene systems never run** (`SceneScheduleError::EngineSet`): `.after(FixedSet::Physics)`, `.in_set(FixedSet::Juice)`, any `PostUpdateSet`. Naming where they already run is fine: `.in_set(FixedSet::Game)`, `.after(FixedSet::Scene)`, `.before(FixedSet::Physics)`. A system that must run after physics (reading this step's contacts or poses) is a builder system: `EngineBuilder::add_systems(Phase::FixedUpdate, react.after(FixedSet::Physics))`.
- **The same line added the same systems in an earlier callback** (`SceneScheduleError::AlreadyAdded`), as a call in `update` would every step. Add a scene's systems once, in `on_enter`; they stay through a push and pop, so `on_resume` needs no second call.

Scene systems see only the world: state in the scene's own fields is out of their reach. Keep what they share with the scene in a resource or on components.

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

<!-- self: score: u32 -->
<!-- with: #[derive(Resource, Default)] struct Score(u32); -->
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

- A message stays readable until two fixed steps have run after it was written, so per-frame and fixed-step readers both see it; each scene and each reader sees it once. Read a type in one place per scene.
- Engine messages: `CollisionStarted`/`CollisionStopped` and `TriggerEntered`/`TriggerExited` (physics), `MovementEvent`, `PickedUp`, `Killed`, `CheckpointReached` (gameplay), `TweenStarted`/`TweenCompleted`, `KnockbackStarted`/`KnockbackFinished`, `SquashStretchStarted`/`SquashStretchFinished`, `ParticleCollision` and `EmitterFinished` (`ryo-particles`), `MapEvent` (a tile map landed, reloaded or failed: `ryo-tilemap`), `ChunkEvent` (a streamed map's chunk landed or unloaded: `ryo-tilemap`), and an `AssetEvent<T>` per asset type (`ryo-assets`). Details: `ryo-messages`.

<!-- self: score: u32 -->
<!-- given: player: Entity -->
```rust
// A coin is a trigger with a Pickup: the engine collects and despawns it
for picked in ctx.messages::<PickedUp>() {
  if picked.by == player {
    self.score += 10;
  }
}
```

## Time

| Clock | What it is | Use |
|-------|------------|-----|
| `ctx.time.real` | Wall clock, never scaled or paused | UI animation, real-time effects |
| `ctx.time.game` | Real delta times `speed`, capped at 250 ms of game time per frame; zero while paused or in hitstop | `set_speed(0.5)`, `pause()`, `resume()`, `is_paused()`, `hitstop(0.08)`, `elapsed()` |
| `ctx.time.fixed` | Game time cut into constant steps | `timestep()`, `overstep_fraction()`, `step_count()` |

- The step rate is `EngineBuilder::fixed_hz(hz)` (default 60). Slow motion runs fewer steps, pause and hitstop run none; `dt` never changes. Physics follows all of it.
- While the game clock is paused or in hitstop **no `update` runs anywhere**; `ui` still runs every frame, so unpause from `ui`.
- Audio follows the game clock: a pause holds the buses that pause with the game, and slow motion lowers the pitch of those that follow time scale (`ryo-audio`, Pause, time scale and focus). Hitstop leaves audio alone, so a hit's sound plays through the freeze. A scene that is not updated is not heard either (Scene changes, below).

## Input

Bind **actions** and read them, rather than raw keys: one name covers the keyboard, the mouse and the pad, the player can rebind it, and prompts show the right key or button.

<!-- with: #[derive(Default)] struct PauseMenu; impl Scene for PauseMenu {} -->
```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  use Binding::{Key, Pad};
  let stick_left = Binding::Stick(StickDirection { stick: Stick::Left, direction: Direction::Left });
  ctx.actions()
    .bind("jump", [Key(KeyCode::Space), Pad(GamepadButton::South)])
    .bind("left", [Key(KeyCode::KeyA), Key(KeyCode::ArrowLeft), stick_left])
    .bind("pause", [Key(KeyCode::KeyP), Pad(GamepadButton::Start)]);
}

fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
  if ctx.input.action_pressed("jump") {}     // this step only
  if ctx.input.action_held("left") {}        // held
  if ctx.input.action_pressed("pause") {
    return SceneAction::Push(Box::new(PauseMenu::default()));
  }
  SceneAction::Continue
}
```

- `bind` sets an action's defaults; bindings the player changed (a `ui.rebind` row, `ctx.actions().rebind(..)`) stay, and binding again is harmless. `ctx.actions().reset_defaults()` puts them back. With `EngineBuilder::with_user_preferences([UserPreference::Bindings])` they are kept between runs in `system.json`, written a second after they change.
- Show what triggers an action with `ui.prompt("jump")` or `[prompt=jump]` in markup: a key on the keyboard, the right button for the pad's family.
- Raw input is still there: `ctx.input.key_pressed(KeyCode::Space)`, `key_down`, `mouse_pressed(MouseButton::Left)`, `mouse_position()` (screen space: `ctx.camera().screen_to_world(Vec2::new(mx, my))` for the world).
- One-shots (`action_pressed`, `key_pressed`, `mouse_pressed`, ...) are seen by the first fixed step of a frame; a press in a frame with no step reaches the next step. Handle a press once, in `update` or in a system.
- The UI pass runs before the fixed steps and consumes what it uses: a click on a button never reaches `update`, a focused text field takes the keyboard, and the arrows, Space and A are the UI's only while a focus scope (a menu) is on screen. Under a HUD with no scope they stay gameplay's.
- A consumed key reads as not pressed and not held for the rest of the frame. `ctx.input.key_pressed_and_consume(key)` reads and consumes in one call.
- **Menus** (title screen, pause menu, settings with rebinding, inventory, dialogue, quit confirmation): build them from the `ryo-ui` recipes, which handle the pad, the keys, the mouse and Back.
- Gamepads, sticks and pad families: `ryo-controller`.

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

<!-- with: use std::time::Duration; #[derive(Default)] struct Level; impl Scene for Level {} -->
```rust
fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
  SceneAction::SwitchWith(
    Box::new(Level::default()),
    Transition::fade(Duration::from_millis(400)),
  )
}
```

- Transitions run on real time, once a frame, so a fade finishes even with the game clock paused or slowed.
- While one runs, `update` is not called, game-time particle emitters are frozen, and a scene change returned from `ui` is ignored (only `Quit` applies): a second click during a fade does not start another.
- A pushed scene that does not `updates_below` stops the scenes below, pauses physics and holds their sounds: their voices on the buses that pause with the game (`sfx`, `voice`, `ambience`), and the voices of the emitters they added, wait where they are until a `Pop` (`holds_audio_below` returning `false` leaves them audible). The pushed scene's own sounds, the music, `persist` voices and `ui` play on. A game-time particle emitter steps only while its scene updates, so the ones these scenes added are frozen until they update again, and one added outside every scene is held while any scene is held; an `unscaled` emitter keeps running. Engine animation and tweens keep running: pause the game clock to freeze them too.
- A pause menu: a transparent pushed scene whose `ui` draws a focus-scoped menu and returns `SceneAction::Pop` on Resume or `ui.back()` (`ryo-ui`'s pause menu recipe). Scenes under it still draw, but only the top scene's widgets take input, so its buttons win the clicks.
- Cleanup on `Switch`/`Reset` (not on `Push`/`Pop`): `SceneCleanup` flags `WORLD`, `CAMERA`, `EFFECTS`, `LIGHTING`, `TIME`, `MUSIC`, `MESSAGES`; `DEFAULT` is all but `MUSIC` (so a track carries on from menu to game), plus `NONE` and `ALL`. Combine with `|`, remove with `-`: `SceneCleanup::DEFAULT - SceneCleanup::WORLD` keeps the entities.
- A scene's console commands and systems leave with it, and so do its sounds as it exits by any action (`Pop` too): its loops stop with their fade, as do its one-shots with more than 2 s left (not on `ui`), and its shorter one-shots finish (`ryo-audio`, Voices).

## Sprites and Anchors

- `Renderable::image(handle)` draws the image with its **centre** on the entity's position (`Anchor::Center`). Rotation and scale happen about the anchor.
- Things laid out as a grid or from a corner (backgrounds, tiles, panels) take `.with_anchor(Anchor::TopLeft)`; things that move, rotate or scale keep the centre.
- `Renderable::tiled(..)` and composite tiles are laid out from the entity's position (the first tile's top-left), whatever the anchor.
- Images: `ctx.assets().load_now::<Image>("player.png")` (an asset path, relative to `assets/`; a `Result`, the image ready to draw), `ctx.assets().add(Image::solid_rect(w, h, r, g, b, a))`, `Image::solid_circle(..)`. The handle keeps the image loaded: hold it (the `Renderable` does), and keep what the whole game uses in a resource (`ryo-assets`). An image not loaded draws a magenta checker in debug builds and nothing in release ones.
- Crisp versus smooth: `.with_filter(Filter::Nearest)` keeps hard texels when scaled (pixel art in a smooth game), `.with_filter(Filter::Linear)` smooths (a portrait in a pixel-art game); `Filter::Default` follows the presentation. Linear sprites drawn small stay smooth (mipmaps).
- Glows and light: `.with_blend(BlendMode::Additive)` adds light (fire, sparks, lasers); `Multiply` darkens, `Screen` lightens (a 0..1 formula: over pixels brighter than white it dims them; use `Additive` for light above white). `.with_lighting(false)` keeps a sprite at full brightness in the dark.

Additive particles and trails (a burst effect, played once: `play_at` despawns its emitter when it has finished):

<!-- given: x: f32, y: f32, spark: Handle<Image> -->
```rust
let sparks = ctx.assets().add(ParticleEffect::sparks(spark).with_blend(BlendMode::Additive).with_lighting(false));
ctx.particles().play_at(&sparks, Vec2::new(x, y));
ctx.world.spawn((Transform::from_xy(x, y), Trail2D::laser(Color::CYAN, 3.0).with_blend(BlendMode::Additive)));
```

Parallax: one entity per layer; every camera (split screen, minimap) lays it out from its own view.

<!-- given: sky: Handle<Image>, hills: Handle<Image> -->
```rust
ctx.world.spawn(ParallaxLayer::new(&sky).with_z_order(-100));
ctx.world.spawn(
  ParallaxLayer::new(&hills).with_scroll(Vec2::splat(0.4)).with_tile(TileMode::InfiniteScroll).with_z_order(-60),
);
```

Debug drawing (one screen pixel wide at any zoom; from `update` it lasts until the next step):

<!-- given: player: Vec2, target: Vec2, enemy: Vec2, sight_range: f32 -->
<!-- with: #[derive(Component)] struct Route { points: Vec<Vec2> } -->
```rust
ctx.debug_draw().line(player, target, Color::RED);
ctx.debug_draw().width(2.0).circle(enemy, sight_range, Color::YELLOW);

// From a system
fn show_paths(mut debug: ResMut<DebugDraw>, agents: Query<&Route>) {
  for route in &agents {
    debug.polyline(&route.points, Color::GREEN);
  }
}
```

## Tile Maps in Brief

Levels are made in Tiled or LDtk and load like any asset; a `MapRoot` entity spawns one, its `Transform` at the map's top-left:

<!-- self: level: Option<Entity> -->
```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  let map = ctx.assets().load::<TileMap>("maps/level1.tmx"); // or LdtkProject and MapRoot::ldtk
  self.level = Some(ctx.world.spawn((Transform::from_xy(0.0, 0.0), MapRoot::new(map))).id());
}
```

- The map lands a frame or so after its files load, whole: its layers (drawn in chunks at the root's z plus their index), its objects as `MapObject` entities, its colliders from the tiles' collision shapes, its shadows, and a `MapEvent::Spawned`. Wire objects and place the player then.
- `ctx.maps()` reads and edits tiles (`tile`, `set_tile`, `fill`, Wang `set_terrain`) and converts between cells and world positions; an edit redraws and re-collides only what it touched.
- Saving the map in its editor updates it while the game runs, keeping what the save did not change. All of it: `ryo-tilemap`.

## Physics in Brief

<!-- given: sprite: Handle<Image> -->
```rust
ctx.world.spawn((
  Transform::from_xy(200.0, 100.0),
  Renderable::image(sprite),
  RigidBody::dynamic(),
  Collider::rect(32.0, 32.0),
));
```

Colliders are centred on the entity's position plus `Collider.offset`, so they line up with a centre-anchored sprite. Bodies are `dynamic()`, `fixed()` or `kinematic()`; a collider with no body is fixed. Bodies may sit anywhere in the hierarchy, and a collider attaches to the nearest body at or above it. Everything else: `ryo-physics`. Players and enemies are characters, moved by a preset or your own code: `ryo-character`.

### Recipe: a platformer player

```rust
use ryo2d::prelude::*;

#[derive(Default)]
struct Level {
  player: Option<Entity>,
  jump: Option<Handle<Sound>>,
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // A bare collider is a fixed floor; a one-way shelf to jump up through
    ctx.world.spawn((Transform::from_xy(400.0, 580.0), Collider::rect(800.0, 40.0)));
    // A sound definition (its clips, bus and variations): ryo-audio
    self.jump = ctx.assets().load_now::<Sound>("sfx/jump.sound.ron").ok();
    ctx.world.spawn((Transform::from_xy(400.0, 500.0), Collider::rect(160.0, 12.0).one_way()));
    let player = ctx
      .world
      .spawn((
        Transform::from_xy(100.0, 540.0),
        Collider::rect(14.0, 26.0),
        // The preset brings the character controller and runs, jumps and falls
        PlatformerMovement::default().with_air_jumps(1),
        // WASD and the arrows move, Space jumps, Shift dashes, plus the first gamepad
        MoveBindings::default(),
      ))
      .id();
    self.player = Some(player);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let Some(player) = self.player else {
      return SceneAction::Continue;
    };
    // What the preset did in the last step
    for event in ctx.messages::<MovementEvent>() {
      if event.entity == player && event.kind == MovementEventKind::Jumped {
        if let Some(jump) = &self.jump {
          ctx.audio().play(jump);
        }
      }
    }
    if let Some(at) = ctx.world.get::<Transform>(player).map(|t| t.translation) {
      ctx.camera().follow(at, 0.15);
    }
    SceneAction::Continue
  }
}
```

The preset writes the character's `Velocity` every step, its own gravity included; the controller sweeps it through the level and leaves the real velocity. Tune it with the builders (`with_run`, `with_jump(height, time_to_apex)`, `with_wall_jump`, `with_dash`, ...); a top-down game uses `TopDownMovement` the same way.

### Recipe: coins, spikes and a checkpoint

```rust
use ryo2d::prelude::*;

#[derive(Default)]
struct Level {
  player: Option<Entity>,
  score: u32,
  lives: u32,
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.spawn((Transform::from_xy(400.0, 580.0), Collider::rect(800.0, 40.0)));
    // Coins: collected (and despawned) by the first thing that touches them
    for x in [250.0, 300.0, 350.0] {
      ctx.world.spawn((
        Transform::from_xy(x, 540.0),
        Collider::circle(6.0).as_trigger(),
        Pickup::new(),
      ));
    }
    // Spikes on the floor send the player back to its Respawn point...
    ctx.world.spawn((
      Transform::from_xy(500.0, 556.0),
      Collider::rect(48.0, 8.0).as_trigger(),
      KillZone::new(),
    ));
    // ...and a flag past them moves that point
    ctx.world.spawn((
      Transform::from_xy(650.0, 540.0),
      Collider::rect(16.0, 40.0).as_trigger(),
      Checkpoint::new(),
    ));
    let player = ctx
      .world
      .spawn((
        Transform::from_xy(100.0, 540.0),
        Collider::rect(14.0, 26.0),
        PlatformerMovement::default(),
        MoveBindings::default(),
        Respawn::here(),
      ))
      .id();
    self.player = Some(player);
    self.lives = 3;
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.score += 10 * ctx.messages::<PickedUp>().len() as u32;
    for killed in ctx.messages::<Killed>() {
      if Some(killed.entity) == self.player {
        self.lives = self.lives.saturating_sub(1);
      }
    }
    if self.lives == 0 {
      return SceneAction::Reset(Box::new(Level::default()));
    }
    SceneAction::Continue
  }
}
```

The behaviours go on trigger colliders and act a step after the overlap; `PickedUp`, `Killed` and `CheckpointReached` reach `update` the step after that. A trigger's layer and mask decide what can set it off.

### Recipe: a stack of crates on a fixed floor

<!-- self: crate_sprite: Handle<Image> -->
```rust
fn on_enter(&mut self, ctx: &mut SceneContext) {
  // A bare collider is a fixed floor
  ctx.world.spawn((Transform::from_xy(400.0, 580.0), Collider::rect(800.0, 40.0)));
  for row in 0..5 {
    ctx.world.spawn((
      Transform::from_xy(400.0, 540.0 - 32.0 * row as f32 - 16.0),
      Renderable::image(self.crate_sprite.clone()),
      RigidBody::dynamic(),
      Collider::rect(32.0, 32.0).with_friction(0.6),
    ));
  }
}
```

The crates settle and fall asleep; a body nobody touches costs nothing. Raise `PhysicsConfig::solver_iterations` if tall stacks wobble.

### Recipe: a rope bridge with a break force

```rust
let left = ctx.world.spawn((Transform::from_xy(100.0, 300.0), RigidBody::fixed())).id();
let right = ctx.world.spawn((Transform::from_xy(730.0, 300.0), RigidBody::fixed())).id();
let mut previous = left;
for i in 1..=10 {
  let plank = ctx.world.spawn((
    Transform::from_xy(100.0 + 60.0 * i as f32, 300.0),
    RigidBody::dynamic(),
    Collider::rect(54.0, 8.0),
  )).id();
  // Hinge at the plank's left end, in the gap shared with the previous plank, breaks under a sustained load
  let hinge = Vec2::new(100.0 + 60.0 * i as f32 - 30.0, 300.0);
  ctx.world.spawn(Joint::revolute(previous, plank).at(hinge).with_break_force(40_000.0));
  previous = plank;
}
// The last plank ends at x = 727; the far anchor sits just beyond it
ctx.world.spawn(Joint::revolute(previous, right).at(Vec2::new(730.0, 300.0)));

// in update
for broken in ctx.messages::<JointBroken>() {
  // play a snap, drop the planks' sprites, tell the player
  let _ = broken.joint;
}
```

`with_break_force` measures a sustained pull; a sudden jerk may break a joint early or not at all, so tune the number by loading the bridge. A broken joint is removed along with its `Joint` component.

### Recipe: a line-of-sight check

```rust
fn can_see(ctx: &SceneContext, looker: Entity, target: Entity) -> bool {
  let (Some(from), Some(to)) = (
    ctx.world.get::<GlobalTransform>(looker),
    ctx.world.get::<GlobalTransform>(target),
  ) else {
    return false;
  };
  let (from, to) = (from.translation(), to.translation());
  // Exclude the looker so its own collider does not block the ray; triggers are skipped by default
  let filter = QueryFilter::new().excluding(looker);
  match ctx.physics().raycast(from, to - from, from.distance(to), &filter) {
    Some(hit) => hit.body == target || hit.collider == target,
    None => true,
  }
}
```

Queries see the world as the last step left it, so a wall spawned this tick does not block yet.

## Hierarchy and Interpolation

<!-- given: ship: Entity, sprite: Handle<Image> -->
```rust
let turret = ctx.spawn_child(ship, (Transform::from_xy(0.0, -12.0), Renderable::image(sprite)));
ctx.world.detach_keep_world(turret); // leave the parent, stay where it is
```

- A child's `Transform` is relative to its parent; reparent by inserting `ChildOf(parent)`. Despawning a parent despawns its children.
- Inserting a `Transform` places the entity at once (its `GlobalTransform` is right for the rest of the step); later edits through `get_mut` are propagated at the end of the step.
- Smooth motion at a fixed rate below the display rate: add `Interpolated` to moving entities (it covers their children). After a teleport call `ctx.skip_interpolation(entity)` so it does not slide.

## Pooling

<!-- given: sprite: Handle<Image>, at: Vec2, spent: Entity -->
```rust
#[derive(Component, Clone)]
struct Bullet;

let mut bullets = Pool::new(
  move || (Bullet, Transform::IDENTITY, Velocity::zero(), Renderable::image(sprite.clone())),
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

Released entities carry `Disabled`: every query, physics and rendering skip them. Its children leave and come back with it, and an acquired entity does not slide from where it was released. `PoolConfig` fields: `initial_capacity`, `max_capacity` (0 = unlimited), `expand_batch_size`, `exhausted_policy` (`Expand`, `ReturnNone`, `RecycleOldest`).

## Cameras and Coordinates

- Cameras are entities (`Camera2d` + `Transform`); `ctx.camera()` edits the main one. Its position is the centre of the view. It starts (and `SceneCleanup::CAMERA` resets it) at the centre of the design area, so world (0, 0) is the design area's top-left and one world unit is one design pixel. +Y is down; rotation is clockwise in radians.
- `set_position`, `follow(target, lerp)`, `snap_to`, `clamp_to_bounds(min, max)`, `set_zoom`, `set_rotation`, `add_trauma(0.4)` (shake, in real time), `screen_to_world`, `world_to_screen`, `visible_bounds()`. Shake, rotation and zoom move everything in the world (sprites, trails, lines, lights, shadows); the UI stays put.
- Follow the player in `update` (or a `FixedUpdate` system) with `ctx.camera().follow(player_pos, 0.1)`: the main camera is `Interpolated`, so it moves as smoothly as the sprite it follows. Give an extra camera `Interpolated` too.
- Clamp a centred sprite with its half size: `t.translation.x.clamp(half_w, screen_w - half_w)`.

Split screen: two cameras, each with half the window.

<!-- given: p2: Vec2 -->
```rust
ctx.camera().set_viewport(Viewport::new(0.0, 0.0, 0.5, 1.0));
let right = ctx.world.spawn((
  Transform::from_xy(p2.x, p2.y),
  Camera2d { projection: Projection::Fit { width: 320.0, height: 360.0 }, viewport: Viewport::new(0.5, 0.0, 0.5, 1.0), order: 1, ..Default::default() },
  Interpolated,
)).id();
// In update: move `right`'s Transform after player two
```

Minimap: a small camera on top that does not clear, seeing only what is on its layer.

<!-- given: x: f32, y: f32, dot: Handle<Image> -->
```rust
ctx.world.spawn((
  Transform::from_xy(0.0, 0.0),
  Camera2d { zoom: 0.1, viewport: Viewport::new(0.78, 0.02, 0.2, 0.2), order: 10, clear: CameraClear::Color(Color::new(0, 0, 0, 160)), ..Default::default() },
  RenderLayers::layer(0).with(1), // the world plus minimap-only markers on layer 1
));
ctx.world.spawn((Transform::from_xy(x, y), Renderable::image(dot), RenderLayers::layer(1)));
```

Render to texture: a camera draws into a target, and a sprite shows it (this frame's image: texture cameras render first).

```rust
let monitor = ctx.render_targets().create(160, 90);
ctx.world.spawn((Transform::from_xy(5000.0, 0.0), Camera2d { target: CameraTarget::Texture(monitor), ..Default::default() }));
ctx.world.spawn((Transform::from_xy(400.0, 120.0), Renderable::render_target(monitor).with_z_order(5)));
```

A camera never draws something showing its own target. A world-clearing scene switch removes every target; recreate them in `on_enter`.

## Resolution and Window

Resolution comes from `Ryo.toml` (`[system] resolution`) or `EngineBuilder::resolution`. `Presentation` says how the world reaches the window:

| `Presentation` | Behaviour |
|--------------|-----------|
| `Fit { width, height }` (a resolution is set) | Screen space (world view, UI, mouse) is the resolution; the frame renders at the window's physical resolution, scaled by any factor to fit and centred with bars where the aspect ratios differ, so it fills a full-screen window and stays sharp |
| `PixelArt { width, height }` | Renders into that target, scaled by a whole number and centred with bars, nearest sampling, sprites snapped to whole target pixels (`EngineBuilder::pixel_art(w, h)` or `pixel_art = true` in `[system]`); screen space is the target |
| `ResolutionIndependent` (no resolution set) | Renders at the window's physical resolution; the main camera keeps 1280x720 world units in view (`Projection::Fit`), so a wider window shows more world at the sides; screen space (UI, mouse) is the window in logical pixels |

Choose a resolution (fit) for most games: one fixed canvas, laid out once, filling any window. Choose pixel art for low-resolution art that must stay crisp and grid-aligned (every pixel the same size). Leave the resolution unset when screen space should be the window's own pixels (a UI that reflows, a tool).

`EngineBuilder::window_size(w, h)` sets the window's initial size and `resizable(bool)` whether the player can resize it. The window's size and display mode are the player's settings: `ctx.settings().set_window_size(w, h)` (logical pixels) and `ctx.settings().set_display_mode(DisplayMode::BorderlessFullscreen)` (`Windowed`, `ExclusiveFullscreen`, `BorderlessFullscreen`), applied after the frame; a resize or fullscreen toggle by the player updates them too. With `UserPreference::Resolution` kept, the window size the player chose is saved and opens the next run, in place of the game's. Builder overrides (`resolution`, `pixel_art`, `window_size`, `target_fps`, `vsync`, `fixed_hz`) beat `Ryo.toml`; the player's saved settings beat both for what `with_user_preferences` keeps (`ryo-config-save`).

## Glow, Mood and the Look

The world renders in linear HDR and `ctx.look()` (a `Mut<Look>`) decides how it reaches the screen. All colours are sRGB; light above 1 is kept until the look rolls it off. A scene switch resets effects and the look (`SceneCleanup::EFFECTS`). Details in `ryo-renderer` and `ryo-api`.

<!-- given: x: f32, y: f32, gem: Handle<Image> -->
```rust
// A glowing pickup: emission above 1, unlit so it glows in the dark, and bloom
ctx.world.spawn((
  Transform::from_xy(x, y),
  Renderable::image(gem).with_emission(4.0).with_lighting(false),
));
ctx.effects().set_bloom(1.0);

// A moody level: cooler, less saturated, crushed blacks, and a LUT
let night = ctx.assets().load_now::<Image>("luts/night.png").ok();
let lut = night.and_then(|strip| ColorLut::from_sprite(&strip, ctx.world.resource::<Assets<Image>>()).ok());
ctx.look().grading = ColorGrading {
  temperature: -0.3,
  saturation: 0.8,
  contrast: 1.2,
  lift: Vec3::splat(-0.02),
  lut, // the LUT holds its image
  lut_strength: 0.6,
  ..ColorGrading::default()
};

// A cave: exposure follows the player from the dark into daylight
ctx.look().auto_exposure = Some(AutoExposure {
  compensation: -0.5,
  ..AutoExposure::default()
});
```

`with_emission` multiplies a drawable's light (also on particle effects, trails and lines). Bloom picks up what is brighter than white (its threshold is linear luminance, default 1.0), so emission above 1 and lights with intensity above 1 glow. A LUT is a strip of N tiles of N x N (`ColorLut::identity_strip(32)` makes a neutral one to paint over).

## Navigation

Nothing in the engine moves navigation agents: the scene owns a `NavigationGrid` and a `Pathfinder` and moves entities along the paths.

```rust
let grid = NavigationBuilder::new(800.0, 600.0)
  .with_cell_size(16.0)
  .build_from_world(ctx.world); // blocks every non-trigger Collider
```

A grid can instead be a window of a tile layer's own cells, on any grid: six neighbours on hex, and each step costing its distance on screen, so an isometric path is the shortest one the player sees.

<!-- given: map: Entity -->
```rust
let Some(layer) = ctx.maps().grid(map, "ground") else {
  return;
};
// The layer's cells from (-32, -32) up to (32, 32): GridCoords are its cells
let grid = NavigationGrid::for_layer(layer, CellRect::new(IVec2::splat(-32), IVec2::splat(32)));
let path = Pathfinder::new().find_path_grid(&grid, GridCoord::new(0, 0), GridCoord::new(10, 5));
```

`grid.block_tiles(&tiles, global.affine())` blocks a map's colliding cells, its `TileColliders` placed by its entity's whole `GlobalTransform` (`rebuild_from_world` does it for every map). `ctx.maps().navigation(map, "ground", window)` does both at once: a grid over the layer's window with every colliding layer of the map blocked, and on a streamed map the chunks not landed (`ryo-tilemap`, section 8). On a map with storeys it gives the grid of that layer's storey: only that storey's walls block, and above the ground only its footprint is open.

**Paths across storeys.** Each `PathNode` carries its `floor` (the grid's, `NavigationGrid::floor`: the ground floor unless the grid was made for another). A `FloorNavigation` is a grid per storey over the same cells, joined at `StairLink`s (a cell where stairs join two storeys, crossed for a thousandth of a step, so a path crosses a run of stairs once); `ctx.maps().floor_navigation(map, window)` takes one from a map's storeys and stairs, and `Pathfinder::find_floor_path` (world points, each with its floor) or `find_floor_path_grid` searches it, in one call (a search across storeys is not time-sliced). The points are in ground space: where a character on that storey stands (its `Transform`), not where it draws, so a click on a storey's art goes through `ctx.maps().floor_cell_at_world` into `find_floor_path_grid` (`ryo-tilemap`, section 8). The path's floor changes between two nodes at a stair cell, so a character following it changes floor where the stairs do; smoothing never skips across that change. A game can build a floor graph without a tile map, from grids it puts on their floors:

```rust
// Two levels of 20 x 15 cells, the upper open only over its gallery,
// joined by a ladder at (4, 7)
let ground = NavigationGrid::new(320.0, 240.0, 16.0);
let mut gallery = NavigationGrid::new(320.0, 240.0, 16.0).on_floor(Floor::On(1));
for y in 0..15 {
  for x in 0..20 {
    if !(4..12).contains(&x) || !(5..9).contains(&y) {
      gallery.set(GridCoord::new(x, y), CellState::Blocked);
    }
  }
}
let ladder = StairLink { cell: GridCoord::new(4, 7), lower: Floor::GROUND, upper: Floor::On(1) };
let levels = FloorNavigation::new(vec![ground, gallery], vec![ladder]);
let path = Pathfinder::new().find_floor_path_grid(
  &levels,
  (Floor::GROUND, GridCoord::new(1, 1)),
  (Floor::On(1), GridCoord::new(10, 6)),
);
for node in path.path().iter().flat_map(|path| path.waypoints()) {
  // Walk to (node.x, node.y), and put the agent on node.floor there
  let _ = (node.x, node.y, node.floor);
}
```

The grids must cover the same cells, as a map's storeys do: the search measures every storey's cells by the lowest's.

`NavAgent::new(speed).with_smoothing(PathSmoothing::LineOfSight)`, `NavTarget::position(x, y)` / `NavTarget::follow(entity)`, and `steering::{separation, seek, flee, arrive, cohesion, alignment}`. See `src/core/navigation/AGENTS.md` and `ryo-api`.

## Visual Tests

`ryo2d::testing::HeadlessEngine` runs a scene without a window, one fixed step and one render per `step`, and returns frames as pixels:

<!-- with: #[derive(Default)] struct Level; impl Scene for Level {} -->
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

- Same scene, steps and input give identical frames: seed your RNG. Particles repeat with no seeding in a headless run; `ParticleEmitter::with_seed(n)` fixes one emitter's stream whatever else the scene adds.
- Pads and menus: `tap_pad(GamepadButton::DPadDown)`, `press_pad`, `set_stick(Stick::Left, x, y)`, `wheel(dx, dy)`, `trackpad(dx, dy)` (logical pixels); `ui_focused()`, `ui_focusables()` and `focus_ui(id)` check pad navigation. `ryo2d::testing::UiHarness` tests a UI function without a GPU (`ryo-ui`, Testing UI).
- Headless runs play sound silently on kira's mock backend (everything works; `ryo-audio`, Testing), and have no real gamepads (the pad methods drive a virtual one); saves and `system.json` stay in a memory store, never on disk (`engine.saves()` plants and reads them); `Ryo.toml` is honoured. `engine.world()` / `world_mut()` reach the world between steps.

## Pitfalls

- Spawning returns `EntityWorldMut`: store `.id()`, not the spawn result.
- Positions are `t.translation.x`, not `t.x`; rotation is radians.
- A sprite placed by its top-left corner needs `.with_anchor(Anchor::TopLeft)`; otherwise it is centred on its position.
- Do not change the world while iterating a query over it: collect entities first.
- Keys the UI acts on: read and consume them in `ui`, or `update` sees them too. Prefer `ui.back()` for Escape and B: it is true once, for the innermost thing that should close.
- A menu without `.focus_scope()` cannot be played with the pad or the arrows.
- No `update` runs while the game clock is paused.
- Integrate your own `Velocity` for entities without a `RigidBody` or `CharacterController`.
