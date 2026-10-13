---
name: ryo-api
description: Complete API reference for ryo2d::prelude - every exported type with its signatures and use, organised by area.
metadata:
  version: 2.0.0
---

# Ryo2D API Reference

Everything `use ryo2d::prelude::*` brings in, with signatures taken from the code. Other skills explain how to use an area; this one is the lookup. When a signature here and the code disagree, the code (and its rustdoc) wins.

Conventions: 2D with +Y down, units are pixels, rotation is radians unless a name says degrees, `Color` is sRGB `u8`. The engine has three clocks and runs gameplay at a fixed rate; see [Time](#time) and [Phases](#phases-and-sets).

```rust
use ryo2d::prelude::*;
```

Contents: [ECS kernel](#ecs-kernel) · [Transform and hierarchy](#transform-and-hierarchy) · [Engine components](#engine-components) · [Juice](#juice-tweens-knockback-squash) · [Pooling](#pooling) · [Time](#time) · [Phases](#phases-and-sets) · [Scenes](#scenes) · [SceneContext](#scenecontext) · [EngineBuilder](#enginebuilder) · [Messages](#messages) · [Input](#input) · [Audio](#audio) · [Cameras](#cameras) · [Sprites](#sprites) · [Sprite animation](#sprite-animation) · [GPU sprites and parallax](#gpu-sprites-and-parallax) · [Composite tiles](#composite-tiles) · [Physics](#physics) · [Lighting](#lighting) · [Assets](#assets) · [Window and cursor](#window-and-cursor) · [Post-processing](#post-processing) · [Trails and lines](#trails-and-lines) · [Debug drawing](#debug-drawing) · [Skeletons](#skeletal-animation) · [Particles](#particles) · [Tilemaps](#tilemaps) · [Render pipeline](#render-pipeline) · [Config and saves](#config-and-saves) · [UI](#ui) · [Errors](#errors) · [Navigation](#navigation) · [State machines](#state-machines) · [Networking](#networking) · [Profiling, console, logging](#profiling-console-logging) · [Optional crates](#optional-crates) · [Testing](#testing-ryo2dtesting)

---

## ECS kernel

Ryo2D's ECS (`ryo2d::ecs`, all in the prelude) is a curated set of kernel types. Write them as Ryo2D types; never name the crate behind them.

| Name | Use |
|------|-----|
| `World` | All entities, components and resources. `ctx.world` in scene callbacks |
| `Entity` | Entity id (`Copy`) |
| `Component`, `Bundle` | Component trait (derive it) and tuples of components to spawn |
| `Resource` | Singleton type stored in the world (derive it) |
| `Commands`, `EntityCommands` | Deferred spawns/inserts from systems, applied at the end of the phase |
| `EntityWorldMut` | What `world.spawn(..)` / `world.entity_mut(e)` return; `.id()` gives the `Entity` |
| `Query`, `Res`, `ResMut` | System parameters |
| `With`, `Without`, `Or`, `Has`, `Added`, `Changed`, `Allow` | Query filters (`Allow<Disabled>` includes disabled entities) |
| `Ref`, `Mut`, `DetectChanges`, `DetectChangesMut` | Change-detecting references (`set_if_neq`, `is_changed`) |
| `Mutable` | Component/resource mutability marker (bound on `resource_mut`) |
| `RemovedComponents` | System param: entities that lost a component |
| `Disabled` | Marker: every query skips the entity unless it asks with `Allow<Disabled>` |
| `IsResource` | Marks the hidden entities that hold resources; filter it out of "every entity" sweeps |
| `ChildOf`, `Children` | Hierarchy relationship (see below) |
| `Message`, `Messages`, `MessageReader`, `MessageWriter`, `MessageCursor` | Buffered messages (see [Messages](#messages)) |
| `Event`, `EntityEvent`, `On`, `Observer` | Observer events: `world.add_observer(\|e: On<T>\| ..)`, `world.trigger(T { .. })` |
| `Schedule`, `ScheduleLabel`, `SystemSet` | Own schedules (tests, tools) |
| `IntoScheduleConfigs`, `IntoSystem`, `System`, `ScheduleSystem` | `.chain()`, `.before()`, `.after()`, `.in_set()`, `.run_if()` on systems |
| `SystemError` | Systems may return `Result<(), SystemError>`; the engine's handler panics in debug and logs in release |
| `Vec2`, `Vec3`, `Vec4`, `Mat4` | glam math types |

Derives: `#[derive(Component)]`, `#[derive(Resource)]`, `#[derive(Message)]`, `#[derive(Event)]`, `#[derive(SystemSet)]`, `#[derive(ScheduleLabel)]`, `#[derive(Bundle)]`.

```rust
#[derive(Component)]
struct Player { speed: f32 }

#[derive(Resource, Default)]
struct Score(u32);

#[derive(Message, Clone)]
struct CoinCollected { value: u32 }

fn move_players(time: Res<Time>, mut players: Query<(&Player, &mut Transform)>) {
  for (player, mut transform) in &mut players {
    transform.translation.x += player.speed * time.delta();
  }
}

fn count_coins(mut coins: MessageReader<CoinCollected>, mut score: ResMut<Score>) {
  for coin in coins.read() {
    score.0 += coin.value;
  }
}
```

World idioms (scene code works on `ctx.world` directly):

```rust
let e = world.spawn((Transform::from_xy(100.0, 50.0), Renderable::sprite(sprite))).id();
world.entity_mut(e).insert(Velocity::new(10.0, 0.0));
if let Some(mut t) = world.get_mut::<Transform>(e) { t.rotation += 0.1; }
let has_velocity = world.get::<Velocity>(e).is_some();
let players: Vec<Entity> = world.query_filtered::<Entity, With<Player>>().iter(world).collect();
for (t, v) in world.query::<(&mut Transform, &Velocity)>().iter_mut(world) { /* .. */ }
let despawned: bool = world.despawn(e); // also despawns descendants

// Resources are entities: a sweep over "everything" must exclude them
let all = world.query_filtered::<Entity, (Allow<Disabled>, Without<IsResource>)>();
```

---

## Transform and hierarchy

```rust
#[derive(Component)] #[require(GlobalTransform)]
pub struct Transform { pub translation: Vec2, pub rotation: f32, pub scale: Vec2 } // local to the parent
```

| `Transform` | |
|---|---|
| `IDENTITY` / `Default` | origin, no rotation, scale 1 |
| `from_xy(x: f32, y: f32) -> Self` | position |
| `from_translation(Vec2) -> Self` | position |
| `with_rotation(radians: f32) -> Self` / `with_rotation_degrees(f32) -> Self` | rotation |
| `with_scale(Vec2) -> Self` / `with_uniform_scale(f32) -> Self` | scale |
| `rotation_degrees(&self) -> f32` / `set_rotation_degrees(&mut self, f32)` | degree helpers |
| `to_affine(&self) -> Affine2` | as a matrix |

`GlobalTransform` (world space, written only by the engine): `IDENTITY`, `from_transform(&Transform)`, `translation() -> Vec2`, `rotation() -> f32`, `scale() -> Vec2`, `affine() -> Affine2`, `transform_point(Vec2) -> Vec2`, `to_transform() -> Transform`, `mul_transform(&Transform) -> Self` (a child's global), `lerp(&Self, t) -> Self`.

- Inserting a `Transform` (spawn or `insert`) sets `GlobalTransform` at once (under the parent's current global for a child). Later edits through `get_mut::<Transform>` reach `GlobalTransform` at propagation: the end of each fixed step (`FixedSet::Transforms`, after physics) and in `PostUpdate`.
- Draw order is `Renderable.z_order`; a sprite's placement is its `Anchor`; there is no `z`, pivot or offset on `Transform`.

Hierarchy (`ChildOf(parent)` / `Children`, plus `trait WorldExt for World`):

| | |
|---|---|
| `world.spawn_child(parent: Entity, bundle: impl Bundle) -> Entity` | spawn under `parent` (also `ctx.spawn_child`) |
| `world.spawn((bundle, ChildOf(parent)))` | the same, by hand; inserting `ChildOf` reparents |
| `world.entity_mut(e).remove::<ChildOf>()` | detach; the local transform becomes the world one |
| `world.detach_keep_world(entity: Entity)` | detach and keep the world position |
| `world.skip_interpolation(entity: Entity)` | draw at the new transform next frame (after a teleport); also `ctx.skip_interpolation` |

Despawning a parent despawns its descendants. Never make an entity its own ancestor.

Interpolation: add the `Interpolated` marker (covers descendants) to draw between the previous and current fixed step. The engine keeps `PreviousGlobalTransform { pub global: GlobalTransform, pub skip: bool }` on those entities. `GlobalTransform` is always the true state; only drawing blends.

Engine systems for tests and tools, outside the prelude: `ryo2d::ecs::propagate::propagate_transforms`, `ryo2d::ecs::interpolate::snapshot_previous_transforms`, `ryo2d::ecs::interpolate::render_global(global: &GlobalTransform, previous: Option<&PreviousGlobalTransform>, alpha: f32) -> GlobalTransform`.

---

## Engine components

### Anchor

`enum Anchor { TopLeft, TopCenter, TopRight, CenterLeft, Center (default), CenterRight, BottomLeft, BottomCenter, BottomRight }`: which point of a sprite (or UI element) sits on its position. `to_pivot(self) -> (f32, f32)` ((0,0) top-left, (1,1) bottom-right); `calculate_position(&self, offset_x, offset_y, element_w, element_h, screen_w, screen_h)` for UI layout. Rotation and scale happen about the anchor.

### Renderable

Sprite or tiled run on an entity with a `Transform`.

```rust
pub struct Renderable {
  pub sprite: SpriteHandle, pub style: RenderStyle, pub visible: bool, pub z_order: i32,
  pub flip_x: bool, pub flip_y: bool, pub alpha: f32, pub tint: Color,
  pub shader: Option<SpriteShader>, pub drop_shadow: Option<DropShadow>,
  pub blob_shadow: Option<BlobShadow>, pub contact_shadow: Option<ContactShadow>,
  pub anchor: Anchor, // default Anchor::Center
  pub target: Option<RenderTarget>, // drawn instead of `sprite` (see render_target)
  pub blend: BlendMode,  // default Alpha (Premultiplied for render_target)
  pub filter: Filter,    // default Filter::Default (the presentation's)
  pub lighting: bool,    // default true; false draws it (and its shadows) after lighting
  pub emission: f32,     // default 1; above 1 it glows past white and feeds bloom
}
pub enum RenderStyle { Single, Tiled { direction: TileDirection, target_size: f32 } }
```

| Method | |
|---|---|
| `sprite(SpriteHandle) -> Self` | single sprite, placed by its anchor |
| `render_target(RenderTarget) -> Self` | shows a render target at its size, placed, tinted, faded, scaled, turned and flipped like a sprite (see [Cameras](#cameras)); blends `Premultiplied` |
| `tiled(SpriteHandle, TileDirection, target_size: f32) -> Self` | repeated to fill `target_size`, laid out from the entity's position (first tile's top-left); ignores `anchor` |
| `with_anchor(Anchor)`, `with_z_order(i32)`, `with_visible(bool)`, `with_flip_x(bool)`, `with_flip_y(bool)`, `with_alpha(f32)`, `with_tint(Color)` | builders |
| `with_shader(SpriteShader)`, `with_drop_shadow(DropShadow)`, `with_blob_shadow(BlobShadow)`, `with_contact_shadow(ContactShadow)` | effects |
| `with_blend(BlendMode)`, `with_filter(Filter)`, `with_lighting(bool)` | how it is drawn (shadows always blend `Alpha`) |
| `with_emission(f32)` | multiplies the sprite's light (default 1). Above 1 it glows past white and feeds bloom; a lit sprite is still darkened by lighting, so to glow in the dark add `with_lighting(false)`. |

Blend and filter modes (on `Renderable`, `ParticleEmitter`, `SpriteDraw`, `ParallaxLayer`, `Trail2D`, `Line2D`):

| `BlendMode` | Result |
|---|---|
| `Alpha` (default) | ordinary transparency |
| `Premultiplied` | the texture's colour is already multiplied by alpha (render targets) |
| `Additive` | adds light: fire, sparks, glows, lasers |
| `Multiply` | darkens what is below by its colour |
| `Screen` | lightens what is below without blowing out; a 0..1 formula, so over pixels brighter than white (HDR light, emission) it dims them, and an emissive Screen drawable over a bright pixel can go below zero and show black: use `Additive` for light above white |

`enum Filter { Default, Nearest, Linear }`: `Default` is nearest under `pixel_art` and linear otherwise. Linear sampling uses mipmaps, so sprites drawn small stay smooth; nearest keeps hard texels.
| `has_shader`, `has_drop_shadow`, `has_blob_shadow`, `has_contact_shadow`, `is_single`, `is_tiled`, `tile_count() -> u32` | queries |

Shadows (all `Default`, builder style):

- `DropShadow { offset: Vec2, color, blur, scale: Vec2 }`: `new()`, `offset(x, y)`, `color(c)`, `blur(f32)`, `scale(x, y)`, `below(offset_y)`, `diagonal(offset)`, `squashed(offset_y, squash)`.
- `BlobShadow { radius_x, radius_y, offset_y, color, height, max_height, height_scaling }`: `new()`, `circle(r)`, `ellipse(rx, ry)`, `radii(x, y)`, `offset(y)`, `color(c)`, `with_height_scale(bool)`, `max_height(f32)`, `set_height(&mut, f32)`, `effective_radius()`, `effective_alpha()`.
- `ContactShadow { radius, intensity, offset_y, color, falloff }`: `new()`, `radius(f32)`, `intensity(f32)`, `offset(y)`, `color(c)`, `falloff(f32)`, `sharp(r)`, `soft(r)`, `alpha_at_distance(d)`.

### Velocity, ZLayer, YSort

- `Velocity { pub dx: f32, pub dy: f32 }` (pixels per second): `new(dx, dy)`, `zero()`, `from_vec2(Vec2)`, `as_vec2()`. Physics integrates it for `RigidBody` entities; nothing integrates it otherwise.
- `ZLayer { Background = -1000, Default = 0, Foreground = 1000, UI = 2000 }`: `as_z_order(self) -> i32`.
- `YSort { foot_offset, sort_offset, depth_scale_enabled, scale_min, scale_max, y_min, y_max }`: sort y-sorted entities by their feet, wherever the anchor is. `new()` (feet at the bottom, `foot_offset` 1.0), `with_foot_offset(0..1)`, `with_sort_offset(px)`, `with_depth_scale(scale_min, scale_max, y_min, y_max)`, `without_depth_scale()`, `calculate_sort_y(top_y, sprite_height)`, `calculate_depth_scale(y) -> (f32, f32)`. Order is by `z_order` first; within one `z_order`, plain sprites draw first, then y-sorted ones by sort y.

### Material, SpriteShader, SpriteEffect

`Material { pub shader: SpriteShader }` is a component; it wins over `Renderable.shader`.

| | Constructors (same set on `Material` and `SpriteShader`) |
|---|---|
| built-in | `outline(Color, thickness: u32)`, `white_outline(u32)`, `glow(Color, radius)`, `flash(Color, intensity)`, `damage_flash(intensity)`, `color_shift(Color, amount)`, `silhouette(Color)`, `shadow()`, `pixelate(block: u32)`, `wave(amplitude, frequency, speed)`, `dissolve(progress)`, `dissolve_with_edge(progress, Color)`, `invert()`, `grayscale(intensity)`, `directional_outline(solid: Color, passable: Color, u32)`, `platform_outline(u32)` |
| custom WGSL | `custom(ShaderHandle)`, `custom_with_params(ShaderHandle, ShaderParams)`, `.with_param(name, impl Into<ParamValue>)` |
| other | `Material::new(SpriteShader)`, `is_builtin()`, `is_custom()`, `update(&mut self, dt) -> bool` |

- `enum SpriteShader { BuiltIn(SpriteEffect), Custom { shader: ShaderHandle, params: ShaderParams } }`; also `as_builtin()`, `shader_handle()`, `params()`, `params_mut()`.
- `enum SpriteEffect { Outline, Glow, Flash, ColorShift, Silhouette, Pixelate, Wave, Dissolve, Invert, Grayscale, DirectionalOutline }` with the constructors above plus `glow_with_intensity`, `hit_flash`, `full_grayscale`; `is_animated()`, `update(dt) -> bool` (advances a `Wave`'s phase; the engine does not call it), `apply_cpu(..)`.
- `ShaderHandle`: `from_source(id, wgsl)`, `id()`, `source()`, `generation()`, `snapshot()`, `replace_source(wgsl)`; load from disk with `ctx.assets.load_shader(path)`.
- `ShaderParams`: `new()`, `set(name, value) -> &mut Self`, `with(name, value) -> Self`, `get(name) -> Option<ParamValue>`. `enum ParamValue { F32, Vec2, Vec3, Vec4, Color }`, `From<f32 | Vec2 | Vec3 | Vec4 | Color>` (a `Color` is sRGB bytes divided by 255, not linear).
- A custom sprite shader's time input is game time (it stops with pause and slows with speed).

`trait Lerp: Clone + Send + Sync + 'static { fn lerp(start: &Self, end: &Self, t: f32) -> Self; }`, implemented for `f32`, `(f32, f32)`, `Vec2`, `Color`, `Rect`.

---

## Juice: tweens, knockback, squash

All run in `FixedSet::Juice` on the fixed step and report through [messages](#messages).

### Tween (component, Transform plus Renderable alpha/tint)

```rust
pub struct Tween {
  pub target_x: Option<f32>, pub target_y: Option<f32>, pub target_rotation: Option<f32>,
  pub target_scale_x: Option<f32>, pub target_scale_y: Option<f32>,
  pub target_alpha: Option<f32>, pub target_tint: Option<Color>,
  pub duration: f32, pub delay: f32, pub easing: Ease, pub loop_mode: TweenLoop, pub speed: f32,
  pub playing: bool, pub just_started: bool, pub just_finished: bool, /* private state */
}
```

| | |
|---|---|
| `new(duration)`, `to_position(x, y, duration)`, `to_rotation(radians, duration)`, `to_scale(sx, sy, duration)`, `to_uniform_scale(s, duration)`, `fade_to(alpha, duration)`, `fade_out(duration)`, `fade_in(duration)`, `tint_to(Color, duration)` | constructors |
| `to_x`, `to_y`, `to_rotation_value`, `to_scale_x`, `to_scale_y`, `to_alpha`, `to_tint` | add targets |
| `with_easing(Ease)`, `with_delay(secs)`, `with_loop(TweenLoop)`, `with_speed(f32)`, `on_start(Fn(f32))`, `on_update(Fn(f32))`, `on_complete(Fn(f32))` | builders |
| `pause`, `resume`, `stop`, `restart`, `set_speed`, `is_finished`, `progress`, `has_started`, `current_alpha`, `current_tint`, `has_transform_targets`, `has_color_targets` | control |
| `update(&mut self, dt, &mut Transform)`, `update_without_transform(dt)`, `apply_color(&mut Renderable)`, `capture_start_renderable(alpha, tint)` | what the engine calls |

- `enum TweenLoop { Once, Loop, PingPong, Times(u32) }`.
- `enum Ease { Linear, In/Out/InOut × Quad, Cubic, Quart, Sine, Expo, Back, Elastic, Bounce }` (e.g. `Ease::OutBack`); `apply(&self, t) -> f32`.
- `TweenSequence { pub playing, pub just_finished, pub loop_sequence, pub speed }`: `new()`, `then(Tween)`, `then_all(iter)`, `with_loop()`, `with_speed(f32)`, `set_speed`, `current()`, `current_mut()`, `current_index()`, `len()`, `is_empty()`, `is_finished()`, `pause`, `resume`, `stop`, `restart`, `advance`. Engine-driven.
- `PropertyTween<T: Lerp>` (component): `new(start, end, duration)`, `with_easing`, `with_delay`, `with_loop(TweenLoop)`, `with_speed`, `on_start`, `on_update`, `on_complete`, `value() -> T`, `progress()`, `playing()`, `is_finished()`, `just_started()`, `just_finished()`, `pause`, `resume`, `stop`, `restart`, `retarget(new_end)`, `update(dt)`. The engine drives `PropertyTween<f32>`, `<Vec2>` and `<Color>`; call `update(dt)` yourself for other types.

```rust
ctx.world.entity_mut(player).insert(
  Tween::to_position(400.0, 180.0, 0.5).with_easing(Ease::OutBack).with_loop(TweenLoop::PingPong),
);
```

### Knockback (component)

`Knockback { pub active, pub just_started, pub just_finished, pub accumulate }`, `DEFAULT_DECAY_RATE = 4.0`.

| | |
|---|---|
| `Knockback::apply(world: &mut World, entity: Entity, direction: Vec2, force: f32)` | start one (adds the component if missing) |
| `Knockback::apply_with_decay(world, entity, direction, force, decay_rate)` | higher decay recovers faster |
| `new(direction: Vec2, force)`, `from_angle(radians, force)`, `with_decay_rate(f32)`, `with_accumulate(bool)` | build one |
| `push(&mut self, direction, force)`, `push_with_decay(..)`, `stop`, `restart`, `update(dt)` | on a component value |
| `current_velocity() -> (f32, f32)`, `strength()`, `force()`, `direction() -> Vec2`, `decay_rate()`, `is_finished()` | state |

A dynamic body (`RigidBody`, not kinematic, with `Velocity`) gets the knockback added to its velocity, so gravity and collisions keep acting; the push is taken back as it decays, except what a collision or drag already absorbed, and `stop()` or removing the component takes it back at once. Anything else is moved through its `Transform`.

### SquashStretch (component)

`SquashStretch { pub active, pub just_started, pub just_finished }`, `enum SquashAxis { Y, X }`.

| | |
|---|---|
| `SquashStretch::apply(world, entity, intensity)` | squash on Y (adds the component if missing) |
| `apply_x(world, entity, intensity)`, `apply_on_axis(world, entity, intensity, SquashAxis)`, `apply_with_decay(world, entity, intensity, decay_rate)` | variants |
| `new(intensity)`, `with_decay_rate`, `with_axis(SquashAxis)`, `with_preserve_volume(bool)`, `with_base_scale(sx, sy)` | build one |
| `squash(&mut self, intensity)`, `squash_with_decay`, `squash_on_axis`, `stop`, `update(dt)`, `capture_base_scale` | on a component value |
| `current_scale()`, `is_active()`, `strength()`, `intensity()`, `base_scale()`, `decay_rate()` | state |

---

## Pooling

```rust
pub struct Pool<B: Bundle>;
Pool::new(factory: impl Fn() -> B + Send + Sync + 'static, config: PoolConfig) -> Self
pool.acquire(&mut self, world: &mut World) -> Option<Entity>   // enabled, interpolation skipped; components keep old values
pool.release(&mut self, world: &mut World, entity: Entity) -> bool // disabled again
pool.release_all(&mut self, world: &mut World)
pool.active(&self) -> impl Iterator<Item = Entity> /* oldest first */; pool.is_active(&self, Entity) -> bool; pool.stats(&self) -> PoolStats
```

- `PoolConfig { initial_capacity: 32, max_capacity: 0 /* unlimited */, expand_batch_size: 16, exhausted_policy: PoolExhaustedPolicy::Expand }`.
- `enum PoolExhaustedPolicy { Expand, ReturnNone, RecycleOldest }`; `PoolStats { total_spawned, active, available }`.
- Pooled entities are `Disabled`, so queries skip them. After `acquire`, set every field you rely on; an `Interpolated` entity draws where you put it this step (acquire skips its interpolation).
- `acquire`, `release` and `is_active` are O(1) plus the entity's children: releasing disables its descendants too, and acquiring brings back the ones it disabled (and skips their interpolation). Entities a scene cleanup despawned are dropped when the pool meets them.

---

## Time

`Time` is a resource and `ctx.time`:

```rust
pub struct Time { pub real: RealClock, pub game: GameClock, pub fixed: FixedClock, /* .. */ }
```

| Clock | Methods | Use |
|---|---|---|
| `real: RealClock` | `delta() -> f32`, `delta_duration()`, `elapsed() -> f32` | wall clock; UI animation, camera shake, timed post effects |
| `game: GameClock` | `delta()`, `delta_duration()`, `elapsed()`, `speed()`, `set_speed(f32)` (clamped to `0..=GameClock::MAX_SPEED`, 100), `pause()`, `resume()`, `is_paused()`, `hitstop(secs)`, `is_hitstop()`, `clear_hitstop()`, `max_delta()`, `set_max_delta(Duration)` (default 250 ms; never below one fixed timestep) | game time: real delta capped, times speed, zero while paused or in hitstop |
| `fixed: FixedClock` | `timestep() -> f32`, `timestep_duration()`, `set_hz(u32)`, `accumulator()`, `overstep_fraction() -> f32` (render alpha, `[0, 1)`), `step_count() -> u64` | accumulates game time into constant steps |

`Time`: `new(fixed_hz)`, `delta() -> f32` (the fixed timestep inside `FixedUpdate`, the game delta elsewhere), `frame_count() -> u64`, `advance(Duration)`, `begin_frame()`, `frame_start()`.

- `Scene::update`'s `dt` is the fixed timestep. Slow motion runs fewer steps, pause and hitstop run none; the step never changes length.
- Hitstop counts down in real time. Physics steps by `fixed.timestep()`, so it follows speed, pause and hitstop.

---

## Phases and sets

`enum Phase { First, PreUpdate, FixedUpdate, Update, PostUpdate, Last }` (`Phase::ALL`). Each frame:

| Step | Runs |
|---|---|
| `First` | advance clocks, publish `Input`, advance message buffers |
| `PreUpdate` | async asset loads, hot reload |
| UI pass | `Scene::ui` for visible scenes, bottom-up; input it consumes never reaches gameplay |
| `FixedUpdate` × 0..N | `FixedSet::Snapshot` → `Juice` → `Scene` (`Scene::update`) → `Game` (scene and game systems) → `Physics` → `Transforms` → `Trails` |
| `Update` | `UpdateSet::Game` (scene per-frame systems) |
| `PostUpdate` | `PostUpdateSet::Transforms` → `Camera` (shake, then every camera's `CameraView`) → `Extract` |
| render | draw list from the world, `Scene::render`, render pipeline |
| `Last` | tracker clearing |

- `enum FixedSet { Snapshot, Juice, Scene, Game, Physics, Transforms, Trails }`, `enum UpdateSet { Game }`, `enum PostUpdateSet { Transforms, Camera, Extract }`: order your systems with `.in_set(..)`, `.before(..)`, `.after(..)`.
- Game-wide systems: `EngineBuilder::add_systems(phase, systems)` (any phase; in `FixedUpdate`/`Update`, a call whose systems are not ordered against anything outside it runs in its `Game` set; one that names a set or another call's system (`.in_set`, `.before`, `.after`) keeps exactly that order). Scene-owned systems: `ctx.add_systems(Phase::FixedUpdate | Phase::Update, systems)`, dropped when the scene leaves the stack.
- Every schedule is single-threaded.

---

## Scenes

```rust
pub trait Scene: 'static {
  fn on_enter(&mut self, ctx: &mut SceneContext) {}
  fn on_exit(&mut self, ctx: &mut SceneContext) {}
  fn on_pause(&mut self, ctx: &mut SceneContext) {}                       // another scene pushed on top
  fn on_resume(&mut self, ctx: &mut SceneContext, returned: Option<Box<dyn Any>>) {}
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction { SceneAction::Continue } // each fixed step
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction { SceneAction::Continue } // once a frame, before the steps
  fn render(&mut self, gpu: &mut GpuRenderContext) {}                    // GPU-batched sprites, particles, parallax
  fn is_transparent(&self) -> bool { false }  // scenes below are drawn and get `ui`
  fn updates_below(&self) -> bool { false }   // scenes below keep updating and running their systems
  fn is_physics_paused(&self) -> bool { false }
  fn cleanup_on_exit(&self) -> SceneCleanup { SceneCleanup::DEFAULT }
  fn name(&self) -> &str { /* type name */ }
}
```

A `SceneAction` returned from `ui` applies at once, before the fixed steps; a scene change from `ui` draws the new scene's UI the same frame.

`enum SceneAction`:

| Variant | |
|---|---|
| `Continue` | default |
| `Push(Box<dyn Scene>)`, `PushWith(Box<dyn Scene>, Transition)` | overlay; the current scene pauses |
| `Pop`, `PopWith(Transition)` | back to the scene below |
| `PopReturn(Box<dyn Any>)`, `PopReturnWith(Box<dyn Any>, Transition)` | pop and hand data to `on_resume` |
| `Switch(Box<dyn Scene>)`, `SwitchWith(..)` | replace the top scene (cleanup runs) |
| `Reset(Box<dyn Scene>)`, `ResetWith(..)` | clear the stack, push one (cleanup runs) |
| `Quit` | exit |

Only the top scene's action applies; a lower scene's (from `update` or `ui`) is dropped with a warning. `SceneAction::kind() -> &'static str` names the variant.

- `downcast_return::<T: Any>(returned: Option<Box<dyn Any>>) -> Option<T>` reads `PopReturn` data in `on_resume`.
- `enum Transition { None, Fade { .. }, Slide { .. } }`: `fade(Duration)`, `fade_color(Duration, r, g, b)`, `fade_white(Duration)`, `slide(Duration, SlideDirection)`, `duration()`. `enum SlideDirection { Left, Right, Up, Down }`.
- `SceneCleanup` bit flags, applied on `Switch`/`Reset`: `NONE`, `WORLD` (despawn every entity, resources stay), `CAMERA`, `EFFECTS` (post-processing effects and the look), `LIGHTING`, `TIME` (game clock speed, pause, hitstop), `MUSIC`, `MESSAGES`, `ALL`, `DEFAULT` (all but `MUSIC`); `|`, `&`, `-`, `!`, `contains`, `is_empty`. Example: `SceneCleanup::DEFAULT - SceneCleanup::WORLD`.

---

## SceneContext

```rust
pub struct SceneContext<'a> {
  pub world: &'a mut World,
  pub time: &'a mut Time,
  pub input: &'a mut Input,
  pub assets: &'a mut AssetManager,
  pub audio: &'a mut Audio,
  /* the scene's schedules, message cursors, console scope */
}
```

`time`, `input` and `assets` are taken out of the world for the callback: use the fields (`ctx.world.resource::<Time>()` panics inside a callback). Systems always see them as resources. Cameras are entities: `ctx.camera()` reaches the main one.

| Helper | |
|---|---|
| `add_systems<M>(&mut self, phase: Phase, systems: impl IntoScheduleConfigs<ScheduleSystem, M>) -> Result<(), SceneScheduleError>` | scene-owned systems; `FixedUpdate` and `Update` only (other phases: `SceneScheduleError::UnsupportedPhase`; `SceneScheduleError` is in the prelude) |
| `messages<T: Message + Clone>(&mut self) -> Vec<T>` | messages written since this scene last read `T` |
| `write_message<T: Message>(&mut self, message: T)` | registers `T` on first use |
| `resource<T: Resource>(&self) -> Option<&T>` | |
| `resource_mut<T>(&mut self) -> Option<Mut<'_, T>>` | |
| `resource_or_default<T: Default>(&mut self) -> Mut<'_, T>` | |
| `insert_resource<T: Resource>(&mut self, value: T)` | |
| `effects(&mut self) -> Mut<'_, EffectsManager>` | post-processing |
| `lighting(&mut self) -> Mut<'_, LightingSettings>` | ambient, atmosphere, forced on/off |
| `look(&mut self) -> Mut<'_, Look>` | exposure, tonemap curve, colour grading (see [The look](#the-look)) |
| `tilemaps(&mut self) -> Mut<'_, TilemapManager>` | |
| `render_pipeline(&mut self) -> Mut<'_, RenderPipeline>` | toggle or insert passes (persist across scenes) |
| `debug_draw(&mut self) -> Mut<'_, DebugDraw>` | debug lines and shapes over the world (see [Debug drawing](#debug-drawing)) |
| `camera(&mut self) -> CameraMut<'_>` | the main camera (respawned over the design area if the game despawned it) |
| `render_targets(&mut self) -> Mut<'_, RenderTargets>` | textures cameras render into and sprites show |
| `window(&mut self) -> Mut<'_, WindowRequest>`, `cursor(&mut self) -> Mut<'_, CursorRequest>` | requests applied after the frame |
| `console(&mut self) -> ConsoleScope<'_>` | scene-scoped console handlers |
| `spawn_child(&mut self, parent: Entity, bundle: impl Bundle) -> Entity` | |
| `skip_interpolation(&mut self, entity: Entity)` | |
| `screen_size() -> (f32, f32)`, `screen_width()`, `screen_height()` | screen space: the window in logical pixels, or the pixel-art target |
| `scale_factor() -> f64`, `display_mode() -> DisplayMode` | from `WindowInfo` |
| `request_display_mode(DisplayMode)`, `request_resolution(w, h)`, `request_display_change(w, h, DisplayMode)`, `request_ui_scale(Option<u32>)` | window changes |
| `set_cursor(CursorStyle)`, `set_cursor_visible(bool)`, `show_cursor()`, `hide_cursor()`, `set_cursor_confined(bool)`, `set_cursor_locked(bool)`, `set_custom_cursor(&SpriteHandle, hot_x: u16, hot_y: u16)`, `set_custom_cursor_centered(&SpriteHandle)` | cursor |

Helpers that return `Mut<..>` borrow the context: chain one call (`ctx.effects().set_bloom(0.6)`) or bind `let mut fx = ctx.effects();` for several.

`ConsoleScope`: `declare(name, description, verbose: Option<impl Into<String>>)`, `register(name, Fn(&[&str], &mut CommandContext) -> CommandResult) -> bool` (removed when the scene leaves the stack), `remove_builtin(name) -> bool`.

A complete scene:

```rust
#[derive(Default)]
struct Level { player: Option<Entity>, particles: ParticleSystem }

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = ctx.assets.create_sprite(Sprite::solid_rect(16, 16, 255, 255, 255, 255), "player");
    let player = ctx.world.spawn((
      Transform::from_xy(320.0, 180.0),
      Renderable::sprite(sprite).with_z_order(10),
      Player { speed: 120.0 },
      RigidBody::new(),
      Collider::box_collider(16.0, 16.0),
      Velocity::zero(),
      Interpolated,
    )).id();
    self.player = Some(player);
    ctx.add_systems(Phase::FixedUpdate, move_players).expect("FixedUpdate takes scene systems");
    ctx.lighting().set_ambient(AmbientLight::night());
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    for hit in ctx.messages::<CollisionStarted>() {
      if let Some(player) = self.player && hit.involves(player) {
        Knockback::apply(ctx.world, player, Vec2::new(-1.0, 0.0), 300.0);
        ctx.time.game.hitstop(0.05);
        ctx.camera().add_trauma(0.4);
      }
    }
    self.particles.update(ctx.world, dt);
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if ui.button("Quit", Rect::new(8.0, 8.0, 80.0, 20.0)) {
      return SceneAction::Quit;
    }
    SceneAction::Continue
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    self.particles.collect_for_render(gpu.world, gpu.batch);
  }
}
```

---

## EngineBuilder

```rust
EngineBuilder::new()
  .title("Game")
  .resolution(640, 360)
  .fixed_hz(60)
  .insert_resource(Score::default())
  .add_message::<CoinCollected>()
  .add_systems(Phase::FixedUpdate, move_players) // FixedSet::Game: it names nothing to order against
  .start_scene(Level::default())
  .build()
  .run()?; // EngineResult<()>
```

| Method | |
|---|---|
| `new()` / `Default` | |
| `title(impl Into<String>)`, `resolution(w: u32, h: u32)`, `icon(impl Into<PathBuf>)` | window |
| `fixed_hz(u32)` | fixed steps per second (default 60, min 1) |
| `target_fps(u32)` | cap when vsync is off |
| `vsync(bool)` | default `true` |
| `background_fps(u32)`, `background_rendering(bool)` | unfocused behaviour |
| `pixel_art(w: u32, h: u32)`, `window_size(w: u32, h: u32)`, `resizable(bool)`, `display_mode(DisplayMode)`, `ui_scale(Option<u32>)` | presentation |
| `show_colliders(bool)`, `show_fps(bool)`, `show_velocities(bool)`, `show_origins(bool)` | debug overlays |
| `with_user_preferences(impl Into<UserPreferenceFilter>)` | let saved `system.json` values override |
| `insert_resource(impl Resource)` | resource for every scene and system |
| `add_systems<M>(Phase, impl IntoScheduleConfigs<ScheduleSystem, M>)` | game-wide systems |
| `add_message::<T: Message>()` | register a message type up front |
| `with_console_commands(impl FnOnce(&mut ConsoleCommandBuilder))` | declare/remove console commands |
| `start_scene(impl Scene + 'static)` | first scene |
| `build(self) -> Engine`, then `Engine::run(self) -> EngineResult<()>` | |

Settings resolve in order: engine defaults, `Ryo.toml` (or values embedded by `ryo2d build`), builder calls, then saved preferences allowed by `with_user_preferences`.

The resolution model: `resolution(w, h)` is the design area. By default (resolution-independent) the world renders at the window's physical size and the main camera's `Projection::Fit` keeps `w`x`h` world units in view however the window is sized; screen space (UI, mouse, `ctx.screen_size()`) is the window in logical pixels. `pixel_art(w, h)` instead renders into a `w`x`h` target scaled by a whole number to fit the window, centred with bars, nearest sampling, sprites snapped to whole target pixels; screen space is the target. `window_size(w, h)` sets the window's starting logical size (by default the resolution; for pixel art the largest whole multiple that fits 1280x720).

`enum Presentation { ResolutionIndependent (default), PixelArt { width, height } }`. `ScreenMapping` converts between window pixels and screen space: `new(presentation, physical, scale_factor)`, `screen_size()`, `scale()`, `window_rect()`, `window_to_screen(Vec2)`, `screen_to_window(Vec2)`, `render_size()`, `ndc_rect()`, `presentation()`.

---

## Messages

Engine messages (all `Message + Clone`):

| Type | Fields | Written when |
|---|---|---|
| `CollisionStarted` / `CollisionStopped` | `entity_a: Entity, entity_b: Entity`; `involves(e) -> bool`, `other(e) -> Option<Entity>` | two colliders start/stop touching |
| `TweenStarted` | `entity` | a `Tween` begins |
| `TweenCompleted` | `entity, finished: bool` | each iteration; `finished` on the last |
| `KnockbackStarted` / `KnockbackFinished` | `entity` | |
| `SquashStretchStarted` / `SquashStretchFinished` | `entity` | |
| `AssetReloaded` | `path: PathBuf` | hot reload (dev builds) |
| `AssetLoadFailed` | `path: PathBuf, error: String` | |
| `ParticleCollision` | see [Particles](#particles) | |

- Scenes: `ctx.messages::<T>()` returns a `Vec<T>` of what arrived since this scene last read `T` (each scene sees each message once); `ctx.write_message(m)`.
- Systems: `MessageWriter<T>::write(m)`, `MessageReader<T>::read()`.
- Buffers advance in `First`, but only once a fixed step has run since the last advance: a message stays readable until two fixed steps have run after it was written, so every reader, per-frame or fixed-step, sees it at any frame rate. While no fixed step runs (the game clock paused) nothing is dropped.
- Game types: `#[derive(Message, Clone)]`; register with `EngineBuilder::add_message::<T>()` or let the first `write_message` do it. See the `ryo-messages` skill.

---

## Input

`Input` is a resource and `ctx.input`. Keys are winit's `KeyCode` (`KeyCode::KeyA`, `Space`, `ArrowLeft`, `Escape`, `Digit1`, ...), buttons `MouseButton` (`Left`, `Right`, `Middle`).

| Area | Methods |
|---|---|
| keyboard | `key_down(KeyCode)`, `key_pressed(KeyCode)`, `key_released(KeyCode)`, `key_repeat(KeyCode)` (press or OS repeat) |
| mouse | `mouse_down(MouseButton)`, `mouse_pressed(..)`, `mouse_released(..)`, `mouse_x()`, `mouse_y()`, `mouse_position() -> (f32, f32)` (screen space: logical pixels or the pixel-art target, outside `0..screen_size` over the bars; `ctx.camera().screen_to_world(Vec2::new(x, y))` for world), `mouse_delta()`, `scroll_delta()` |
| text and IME | `text_input() -> &str`, `has_text_input()`, `consume_text_input()`, `ime_preedit()`, `ime_preedit_cursor()`, `ime_composing()`, `ime_enabled()` |
| gamepad | `gamepad_connected()`, `gamepad_name()`, `gamepad_button_down/pressed/released(GamepadButton)`, `left_stick()`, `right_stick()` (deadzoned, plus `_x`/`_y`), `left_stick_raw()`, `right_stick_raw()`, `left_trigger()`, `right_trigger()` (0..1) |
| consumption | `consume_key(KeyCode)`, `consume_mouse(MouseButton)`, `consume_gamepad_button(..)`, `consume_text()`, `consume_scroll()`, `key_pressed_and_consume(..) -> bool`, `mouse_pressed_and_consume(..)`, `gamepad_button_pressed_and_consume(..)`, `consume_key_press`, `consume_mouse_press`, `consume_gamepad_button_press`, `clear_keyboard_state()` |
| gating | `block_pointer()`, `block_all()`, `unblock()` |
| engine plumbing | `new()`, `queue(InputEvent)`, `begin_frame()`, `set_fixed_step(u32)`, `end_fixed_steps(bool)`, `fixed_step()` |

- One-shots (`*_pressed`, `*_released`) are seen once per frame by per-frame code and by the first fixed step of a frame; a press in a frame that runs no step (except while the game clock is paused, when it is dropped) waits for the next step.
- A consumed input reads as not pressed (held state included) for the rest of the frame. The UI pass consumes what widgets use before `update` runs.
- `enum GamepadButton { South, East, West, North, LeftBumper, RightBumper, LeftTrigger, RightTrigger, LeftStick, RightStick, Select, Start, Guide, DPadUp, DPadDown, DPadLeft, DPadRight }`.
- `enum InputEvent { Key { key, pressed }, Text(String), MouseButton { button, pressed }, MouseMove { x, y }, RawMouseMotion { dx, dy }, Scroll(..), ImeEnabled, ImeDisabled, ImePreedit { text, cursor }, ImeCommit(String), GamepadConnected { id, name }, GamepadDisconnected(id), GamepadButton { id, button, pressed }, GamepadTrigger { id, button, value } }`: what the window, gamepads, `RYO_INPUT` scripts and the headless harness queue.

---

## Audio

`ctx.audio: &mut Audio` works with or without a device (without one, `Result` methods return `AudioError::Unavailable`). `AudioManager` is the device-backed implementation; `Audio::manager()` exposes it.

| `Audio` / `AudioManager` | |
|---|---|
| `load_sound(name: &str, path: impl AsRef<Path>) -> AudioResult<()>` | cache a sound by name |
| `play_sound(name) -> AudioResult<SoundHandle>`, `play_sound_with(name, SoundOptions)` | |
| `play_music(path) -> AudioResult<MusicHandle>`, `play_music_with(path, MusicOptions)`, `play_music_once(path, MusicOptions) -> AudioResult<bool>` (only if none playing) | |
| `crossfade_music(&mut MusicHandle, new_path, Duration)`, `crossfade_music_with(.., MusicOptions)` | |
| `set_master_volume`, `set_sfx_volume`, `set_music_volume` (0..1) and getters | categories |
| `has_sound`, `unload_sound`, `clear_sounds`, `is_music_playing`, `stop_music`, `stop_music_fade(Duration)`, `current_music() -> Option<&mut MusicHandle>` | |
| `Audio::new(Option<AudioManager>)`, `is_available()`, `manager() -> Option<&mut AudioManager>`; `AudioManager::new() -> AudioResult<Self>` | |

- `SoundOptions { volume, panning, playback_rate }`: `new()`, `volume(f32)`, `panning(-1..1)`, `playback_rate(f32)`.
- `MusicOptions { volume, panning, playback_rate, looping: true, fade_in, fade_out_on_drop }`: `new()`, `volume`, `panning`, `playback_rate`, `looping(bool)`, `once()`, `fade_in(Duration)`, `fade_out_on_drop(Duration)`.
- `SoundHandle`: `stop`, `pause`, `resume`, `set_volume`, `set_panning`, `set_playback_rate`, `is_playing`, `is_finished`.
- `MusicHandle`: the same plus `fade_to(volume, Duration)`, `fade_out(Duration)`, `fade_out_and_stop(Duration)`, `update_category_volume(f32)`; dropping it stops the music (with its `fade_out_on_drop`).

See `ryo-audio`.

---

## Cameras

Cameras are entities: a `Camera2d` with a `Transform` (its position is the view's centre in world space, its rotation turns the view). The engine spawns the main camera (`MainCamera`, `Interpolated`) at the centre of the design area with `Projection::Fit` over it; it survives world cleanup, and `SceneCleanup::CAMERA` resets its position, zoom, rotation, projection and shake. Spawn more cameras for split screen, minimaps or render-to-texture.

```rust
pub struct Camera2d {
  pub zoom: f32,              // default 1.0 (clamped to 0.01..=100)
  pub projection: Projection, // default ScreenUnits
  pub viewport: Viewport,     // default Viewport::FULL
  pub order: i32,             // render order within its kind; default 0
  pub target: CameraTarget,   // default Window
  pub clear: CameraClear,     // default Background
}
pub enum Projection { ScreenUnits, Fit { width: f32, height: f32 } } // ScreenUnits: one world unit per screen unit at zoom 1; Fit: keep width x height world units in view
pub struct Viewport { pub min: Vec2, pub max: Vec2 } // fractions of the window: Viewport::FULL, Viewport::new(x, y, width, height)
pub enum CameraTarget { Window, Texture(RenderTarget) }
pub enum CameraClear { Background, Color(Color), None } // None draws over the cameras below
```

Render order: texture cameras first, then window cameras, each by `order` (ties by entity). Window cameras land in their viewports in that order, so a later camera that does not clear (`CameraClear::None`) draws over the earlier ones. Every camera is lit, and shake, rotation and zoom move everything it draws (sprites, trails, lines, lights, shadows, god rays). UI stays in screen space.

`CameraMut` (`ctx.camera()`, or `CameraMut::main(world) -> Option<CameraMut>` where a system holds `&mut World`) edits the main camera:

| | |
|---|---|
| `entity()`, `position() -> Vec2`, `set_position(x, y)`, `zoom()`, `set_zoom(f32)`, `rotation()`, `set_rotation(radians)`, `set_projection(Projection)`, `set_viewport(Viewport)` | the camera |
| `follow(target: Vec2, lerp)` (0.05..0.2 smooth), `snap_to(Vec2)`, `clamp_to_bounds(min: Vec2, max: Vec2)` | movement |
| `add_trauma(f32)`, `configure_shake(max_offset, decay_rate)`, `set_shake_curve(Ease)`, `trauma()` | shake (decays in real time) |
| `view() -> CameraView`, `world_to_screen(Vec2) -> Vec2`, `screen_to_world(Vec2) -> Vec2`, `visible_bounds() -> CameraBounds` | coordinates, as the camera stands now |

Other cameras are edited through their components (`Camera2d`, `Transform`, `CameraShake`). Move a camera that follows the player in `update` (or a `FixedUpdate` system) and keep it `Interpolated`, so it moves as smoothly as the sprites it follows.

- `CameraShake` (component, trauma-based): `new(max_offset, decay_rate)`, `with_intensity_curve(Ease)`, `add_trauma(f32)`, `configure(max_offset, decay_rate)`, `trauma()`, `reset()`, `offset() -> Vec2`, `set_curve(Ease)`, `curve()`.
- `CameraView` (component the engine computes each frame in `PostUpdateSet::Camera` from the interpolated transform): `view_proj: Mat4`, `center`, `rotation`, `scale` (screen units per world unit), `shake`, `rect_min`, `rect_size` (the viewport in screen units), `bounds: CameraBounds` (encloses a rotated view); `world_to_screen(Vec2) -> Vec2`, `screen_to_world(Vec2) -> Vec2`, `is_visible(min: Vec2, max: Vec2) -> bool`.
- `CameraBounds { left, right, top, bottom }`: `width()`, `height()`, `contains(x, y)`.
- `RenderLayers` (component on drawables and cameras): `layer(n)` (0..=31), `all()`, `none()`, `with(n)`, `intersects(&other)`. Default `layer(0)`. A camera draws the entities, composite tiles, trails and lines that share a layer with it; `Scene::render` sprites, particles, parallax and debug overlays are on layer 0.

Render targets: `RenderTargets` (resource, `ctx.render_targets()`) records textures; the GPU textures follow at the next frame.

| `RenderTargets` | |
|---|---|
| `create(width, height) -> RenderTarget` | sides clamped to `1..=MAX_TARGET_SIDE` (8192), with a warning |
| `resize(target, width, height) -> bool`, `remove(target) -> bool` | `false` for an unknown target |
| `size(target) -> Option<(u32, u32)>`, `contains(target) -> bool` | |

A camera with `CameraTarget::Texture(target)` draws into its viewport's part of that target (all of it by default; the rest keeps what was drawn there, so several texture cameras can share a target), one world unit per target pixel at zoom 1 with `ScreenUnits`; one whose target is missing draws nothing and warns once. `Renderable::render_target(target)` shows it; because texture cameras render first, the sprite shows this frame's image. A camera never draws something showing its own target (it skips it and warns once per entity). A scene switch or reset that clears the world removes every target.

```rust
// A minimap in the top-right corner, and a render-to-texture monitor
ctx.world.spawn((
  Transform::from_xy(0.0, 0.0),
  Camera2d { zoom: 0.2, viewport: Viewport::new(0.75, 0.0, 0.25, 0.25), order: 1, ..Default::default() },
));
let screen = ctx.render_targets().create(128, 96);
ctx.world.spawn((Transform::from_xy(2000.0, 0.0), Camera2d { target: CameraTarget::Texture(screen), ..Default::default() }));
ctx.world.spawn((Transform::from_xy(400.0, 200.0), Renderable::render_target(screen)));
```

## Sprites

`Sprite` (CPU image data):

| | |
|---|---|
| `from_rgba(Vec<u8>, w, h)`, `solid_rect(w, h, r, g, b, a)`, `solid_circle(diameter, r, g, b, a)`, `solid_ellipse(w, h, r, g, b, a)` | build |
| `load(path) -> SpriteResult<Self>` (PNG, JPEG, SVG, ...), `load_svg(path, scale)`, `load_svg_fit(path, max_w, max_h)`, `from_svg_data(&[u8], scale)`, `from_svg_data_fit(..)` | load |
| `width()`, `height()`, `data() -> &[u8]`, `is_opaque()`, `scaled(f32)`, `scaled_xy(sx, sy)`, `draw(..)`, `draw_scaled(..)`, `draw_scaled_xy(..)` | |

Wrap a `Sprite` in a `SpriteHandle` for rendering: `ctx.assets.create_sprite(sprite, "name")` or `SpriteHandle::from_sprite(sprite, "name")`.

`AnimatedSprite` (CPU sheet player): `from_sheet(Sprite, frame_count, fps)`, `update(dt)`, `draw(..)`, `frame_size()`.

`Gpu { pub device: Arc<wgpu::Device>, pub queue: Arc<wgpu::Queue> }` is a resource once the GPU is up.

---

## Sprite animation

| Type | API |
|---|---|
| `Animation` | `new(name, frames: Vec<SpriteHandle>, frame_duration: f32)`, `with_frame_durations(name, frames, durations: Vec<f32>)`, `with_loop_mode(LoopMode)`, `name()`, `duration()`, `frame_count()`, `frame(i)`, `frame_duration(i)`, `loop_mode()` |
| `Animator` (component, engine-driven) | `new(Animation)`, `set_animation(Animation)`, `play`, `pause`, `stop`, `reset`, `set_speed(f32)`, `speed()`, `is_playing()`, `just_finished()`, `current_sprite()`, `current_frame_index()`, `animation_name()`, `update(dt)` |
| `AnimationController` (component, engine-driven) | `new()`, `add_animation(AnimationId, Animation)`, `set_state(AnimationId)`, `transition_after_finish(AnimationId)`, `current_state()`, `just_finished()`, `current_sprite()`, `reset`, `pause`, `play`, `set_speed`, `update(dt)` |
| `ValidatedAnimator` (engine-driven on an entity, like `Animator`) | `new(Animation, AnimationId)`, `with_animation(Animation, AnimationId)`, `with_transition(from, to)`, `with_bidirectional(a, b)`, `with_transition_from_any(to)`, `set_animation(id) -> bool`, `force_animation(id)`, `transition_after_finish(id) -> bool`, `current()`, `current_sprite()`, `just_entered()`, `just_finished()`, `can_transition_to(id)`, `time_in_state()`, `pause`, `play`, `set_speed`, `reset`, `update(dt)` |

`type AnimationId = &'static str`; `enum LoopMode { Once, Loop, PingPong, Times(u32) }`; `enum PlaybackState { Playing, Paused, Stopped }`. The engine advances `Animator`, `AnimationController` and `ValidatedAnimator` each fixed step and puts the current frame on the entity's `Renderable`. See `ryo-animation`.

---

## GPU sprites and parallax

`Scene::render(&mut self, gpu: &mut GpuRenderContext)` adds batched sprites for the frame; they sort with entities by `z_order`.

```rust
pub struct GpuRenderContext<'a> { pub batch: &'a mut BatchCollector, pub world: &'a World, /* .. */ }
```

| `GpuRenderContext` | |
|---|---|
| `add_sprite(&SpriteHandle, instance: SpriteInstance, draw: SpriteDraw)` | one sprite, in world space (the instance's centre is a world position; for something fixed on screen, place it with `camera().screen_to_world`); its texture uploads when first drawn and is dropped 600 frames after it was last drawn |
| `add_custom_sprite(&SpriteHandle, &ShaderHandle, &ShaderParams, SpriteInstance, SpriteDraw)` | one sprite with a game shader |
| `screen_size()`, `screen_width()`, `screen_height()`, `camera() -> &CameraView` (the main camera's view) | |

- `SpriteInstance { transform, uv_rect, tint, effect_params, effect_extra, uv_bounds }`: `new()`, `with_transform(x, y, rotation, width, height)` (the quad's centre and size in world units), `with_uv(u_min, v_min, u_max, v_max)`, `with_tint(Color, alpha)`, `with_effect(&SpriteEffect)`, `with_emission(f32)`, `emission() -> f32` (default 1; kept in the transform's last lane as `emission - 1`).
- `SpriteDraw { z: i32, blend: BlendMode, filter: Filter, lit: bool, effects: bool, emission: f32 }` (`Default`: z 0, `Alpha`, `Filter::Default`, lit, no effects, emission 1; `effects` when the instance carries `with_effect`; `with_emission(f32)` multiplies the instance's emission): `SpriteDraw { z: 5, blend: BlendMode::Additive, ..Default::default() }`.
- `BatchCollector`: `new()`, `with_capacity(n)`, `add(&SpriteHandle, SpriteInstance, SpriteDraw)`, `sprite_count()`, `clear()`.

Parallax layers (components; spawn one entity per layer):

- `ParallaxLayer { sprite, scroll: Vec2, auto_scroll: Vec2, offset: Vec2, tile: TileMode, scale, tint, z_order, blend, filter, lighting, scrolled: Vec2 }`: `new(&SpriteHandle)` (fixed on screen at its top-left, untiled, scale 1, white, lit), `with_scroll(Vec2)` (per axis: 0 stays on screen, 1 moves with the world), `with_auto_scroll(Vec2)` (world units per second, game time), `with_offset(Vec2)` (positive moves it right and down), `with_tile(TileMode)`, `with_scale(f32)`, `with_tint(Color)`, `with_z_order(i32)`, `with_blend(BlendMode)`, `with_filter(Filter)`, `with_lighting(bool)`. `scrolled` is how far it has auto-scrolled (set it to carry a backdrop across scenes).
- Every camera on its `RenderLayers` lays it out from its own view, so split screen and minimaps each get their own parallax.
- `enum TileMode { None, Horizontal, InfiniteScroll }`.

---

## Composite tiles

Multi-part sprites laid out from the entity's position (parts at offsets from it, not anchored).

- `CompositeTileBuilder`: `new()`, `cap(sprite, offset_x, offset_y)`, `cap_flipped_y(..)`, `cap_flipped(sprite, x, y, flip_x, flip_y)`, `static_part(..)`, `body_tiled(sprite, x, y, TileDirection, target_size)`, `body_tiled_flipped(..)`, `tiled_part(..)`, `z_order(i32)`, `visible(bool)`, `build() -> CompositeTile`, `build_with_collider() -> (CompositeTile, GroupedCollider)`.
- `CompositeTile { parts: Vec<TilePart>, visible, z_order }` (component): `new()`, `add_part(TilePart)`, `with_visible`, `with_z_order`, `bounds() -> (f32, f32, f32, f32)`, `size()`.
- `TilePart { sprite, offset_x, offset_y, tiling: Option<(TileDirection, f32)>, flip_x, flip_y }`: `static_part(sprite, x, y)`, `tiled_part(sprite, x, y, direction, target_size)`, `with_flip_x`, `with_flip_y`, `is_tiled()`, `bounds()`.
- `GroupedCollider { width, height, offset_x, offset_y, is_trigger, restitution, friction }` (component; physics treats it like a box collider whose top-left is `offset` from the origin): `new(w, h)`, `from_composite(&CompositeTile)`, `with_offset(x, y)`, `as_trigger()`, `with_restitution(f32)`, `with_friction(f32)`.
- `enum TileDirection { Up, Down, Left, Right }`: `offset(tile_size)`, `is_horizontal()`, `is_vertical()`.

---

## Physics

Rapier2D. An entity is simulated when it has `RigidBody` + `Transform` + (`Collider` or `GroupedCollider`) and no parent. 100 px = 1 m; gravity is in px/s².

| Type | API |
|---|---|
| `RigidBody` (component) `{ gravity_scale, drag, is_kinematic, ccd_enabled }` | `new()` (dynamic), `kinematic()` (moved by its `Transform`), `with_gravity_scale(f32)`, `with_drag(f32)`, `with_ccd()` |
| `Collider` (component) `{ shape, offset: Vec2, is_trigger, layer: u32, mask: u32, restitution, friction }` | `box_collider(w, h)`, `circle_collider(r)`, `with_offset(x, y)`, `as_trigger()`, `with_layer(u32)`, `with_mask(u32)`, `with_restitution(f32)`, `with_friction(f32)`, `should_collide_with(&Collider)`, `world_center(position: Vec2) -> Vec2`, `world_bounds(position) -> (Vec2, Vec2)` |
| `enum ColliderShape` | `Box { width, height }`, `Circle { radius }`; centred on the entity's origin plus `offset` |
| `PhysicsConfig` (resource) `{ gravity: (f32, f32) }` | `new()` (0, 980), `with_gravity(gx, gy)`, `zero_gravity()` |
| `RapierPhysicsWorld` (resource) | queries: `are_intersecting(a, b)`, `get_current_intersections(e) -> Vec<Entity>`, `body_velocity(e)`, `body_position(e)`, `is_sleeping(e)`, `is_registered(e)`, `registered_entities()`, `body_count()`; the engine calls `register_entity`, `step`, `set_transform`, `set_velocity`, `read_back`, `drain_collision_events` |
| `PhysicsCollisionEvent { entity_a, entity_b, event_type: CollisionEventType, is_sensor }`, `enum CollisionEventType { Started, Stopped }` | Rapier's raw events; the engine turns them into `CollisionStarted` / `CollisionStopped` messages each step |

- Collider geometry is centred on the entity's origin, which is the sprite's centre with the default `Anchor::Center`.
- Bodies do not rotate. Move a dynamic body through `Velocity`; a game write to `Transform` teleports it. Physics writes position and velocity back after each step.
- Physics pauses while the top scene's `is_physics_paused()` is true. Read collisions with `ctx.messages::<CollisionStarted>()`. See `ryo-physics`.

---

## Lighting

Lighting turns on once any `Light2D` exists (or when forced) and lights the world, never the UI.

| Type | API |
|---|---|
| `LightingSettings` (resource; `ctx.lighting()`) | `set_enabled(bool)` (force on, e.g. a dark scene before any light, or off), `enabled() -> Option<bool>`, `ambient() -> &AmbientLight`, `set_ambient(AmbientLight)`, `atmosphere() -> &Atmosphere`, `set_atmosphere(Atmosphere)` |
| `Light2D` (component) | `point(Color, radius)`, `point_with_intensity(..)`, `point_full(..)`, `spot(Color, radius, angle_deg, direction_deg)`, `spot_with_intensity(..)`, `spot_full(..)`; presets `torch(r)`, `moonlight(r)`, `lamp(r)`, `flashlight(r, dir_deg)`, `warning(r)`, `magic(r, Color)`; builders `with_shadows()`, `with_soft_shadows(f32)`, `with_shadow_color(Color)`, `with_intensity`, `with_falloff`, `with_volumetric()`, `with_god_rays()`, `with_volumetric_and_god_rays()`, `with_volumetric_settings(VolumetricSettings)`; getters/setters for color, radius, intensity, direction, angle, shadows, softness, volumetric |
| `LightOccluder2D` (component) | `polygon(Vec<Vec2>)` (≥ 3 vertices, CCW), `rect(w, h)` (centred), `rect_offset(x, y, w, h)`, `circle(r, segments)`, `line(start, end, thickness)`, `vertices()`, `edges()`, `is_enabled()`, `set_enabled(bool)`, `with_enabled(bool)`, `transform_vertices(x, y, rotation)` |
| `AmbientLight { color, intensity }` | `new(Color, f32)`, `with_intensity(f32)`, presets `dark()`, `night()`, `moonlit()`, `indoor()`, `daylight()`, `sunset()`, `set_intensity`, `set_color`, `with_custom_intensity`, `effective_color()` |
| `Atmosphere { density, color, scattering, god_rays_* }` | `new(density, Color)`, presets `clear()`, `hazy()`, `dusty()`, `foggy()`, `smoky()`, `underwater()`, `with_god_rays()`, `with_god_rays_config(..)`, `with_god_rays_quality(GodRaysQuality)`, `with_density`, `with_color`, `with_scattering` |
| `GodRaysQuality { Low, Medium, High }` | `samples()`, `decay()` |
| `VolumetricSettings { enabled, intensity, density, scattering, god_rays }` (per light) | `new(intensity, density, scattering)`, `enabled()`, `god_rays_only()`, `subtle_beam()`, `strong_beam()`, `flashlight()`, `with_god_rays`, `with_intensity`, `with_density`, `with_scattering` |

Lights and occluders sit at their entity's `GlobalTransform`. A light with a `RenderLayers` component on its entity (default `layer(0)`) lights only the cameras on those layers. Lights are batch quads with no cap; each lit camera has its own light buffers, so lights turn, zoom and shake with the camera, and each shadow darkens only its own light. Colours are sRGB: `Color::to_linear() -> [f32; 4]` is the linear-light, straight-alpha form the GPU gets. Light `intensity` above 1 shows (the look rolls it off); `Atmosphere::god_rays_threshold` is linear luminance (default 1.0). Removed: `with_raymarched_shadows` (use `with_soft_shadows`), and the engine-internal `LightingRenderer`, `LightingParams`, `MAX_LIGHTS` and the shadow geometry exports. See `ryo-lighting`.

---

## The look

`Look` is a resource (`ctx.look()`): how the frame's linear HDR image becomes the picture, applied once per frame by the `tonemap` pass. `SceneCleanup::EFFECTS` resets it.

| Type | API |
|---|---|
| `Look { exposure: f32, tonemap: Tonemap, grading: ColorGrading, auto_exposure: Option<AutoExposure> }` | `Default`: exposure 0 EV (+1 doubles the light), `Tonemap::Soft { knee: 0.8, white: 4.0 }`, neutral grading, no auto-exposure. Order: exposure (plus the auto EV), white balance, contrast and saturation, the curve, lift/gamma/gain, then the LUT in sRGB |
| `enum Tonemap { Soft { knee, white }, Clamp, Aces, AgX }` | `apply(self, Vec3) -> Vec3` (linear in after exposure, linear display colour out). `Soft` is identity up to `knee`, then reaches 1 at `white`, keeping hues; `Clamp` clips at 1 (the pre-HDR look); `Aces` is contrasty; `AgX` is soft with hue-faithful highlights |
| `ColorGrading { temperature, tint, contrast, saturation, lift: Vec3, gamma: Vec3, gain: Vec3, lut: Option<ColorLut>, lut_strength }` | `Default`: 0, 0, 1, 1, `Vec3::ZERO`, `Vec3::ONE`, `Vec3::ONE`, none, 1.0. `temperature` and `tint` are -1 to 1; `contrast` pivots on mid-grey 0.18; `lift`, `gamma` and `gain` work per channel on the display colour |
| `ColorLut` | `from_sprite(&SpriteHandle) -> Result<ColorLut, LutError>`, `size() -> u32`, `identity_strip(size: u32) -> Sprite` (N from 2 to 64). A strip is N tiles of N x N side by side (N*N wide, N high; 1024x32 for N = 32): tile `b` holds blue `b / (N - 1)`, red grows to the right and green downward. Looked up in sRGB; follows its sprite's hot reload |
| `enum LutError { WrongSize { width, height } }` | the sprite is not a strip |
| `AutoExposure { min_ev, max_ev, compensation, speed_up, speed_down }` | `Default`: -4, 4, 0, 3, 1 (EV; speeds in EV per second, real time). Measured on the GPU from the scene (the average luminance without its darkest and brightest 5%) and brought to mid-grey; added to `exposure` |

```rust
ctx.look().exposure = 0.5;
ctx.look().tonemap = Tonemap::Aces;
```

---

## Assets

`AssetManager` is a resource and `ctx.assets`. The engine finishes async loads and hot reloads in `PreUpdate`.

| `AssetManager` | |
|---|---|
| `load_sprite<P: AsRef<Path>>(path) -> AssetResult<SpriteHandle>` | cached by path |
| `load_shader(path) -> AssetResult<ShaderHandle>` | WGSL, hot-reloadable |
| `create_sprite(Sprite, name) -> SpriteHandle` | from generated pixels |
| `load_sprite_async(path) -> AsyncSpriteHandle` | background load |
| `start_loading(paths)`, `loading_progress() -> LoadingProgress`, `is_loading_complete()`, `reset_loading_progress()` | batch loading screens |
| `enable_hot_reload(watch_paths) -> AssetResult<()>`, `disable_hot_reload()`, `is_hot_reload_enabled()` | dev builds |
| `new()`, `with_base_path(p)`, `set_base_path(p)`, `base_path()`, `is_sprite_cached(p)`, `sprite_count()`, `gc_sprites()`, `clear_sprite_cache()`, `cache_sprite_handle(h)`, `stats() -> AssetStats`, `pending_load_count()`, `has_pending_loads()`, `process_async_loads()`, `process_reloads()` | |

- `SpriteHandle` (cheap clone, shared): `new(Sprite, PathBuf)`, `from_sprite(Sprite, name)`, `width()`, `height()`, `path()`, `id() -> String` (texture id for batching), `sprite()` (read guard), `with_sprite(|s| ..)`, `ref_count()`.
- `AsyncSpriteHandle`: `state() -> LoadingState`, `is_loading()`, `is_complete()`, `is_loaded()`, `is_failed()`, `get() -> Option<SpriteHandle>`, `get_or(&fallback)`, `path()`. `enum LoadingState { Loading, Loaded, Failed(String) }`.
- `LoadingProgress { total, completed, succeeded, failed, current, failures }`: `percentage()`, `is_loading()`, `is_complete()`, `current_asset()`, `has_failures()`, `none()`.
- `AssetStats { sprites_cached, sprites_total_refs, sprites_loading }`.

---

## Window and cursor

- `WindowRequest { width, height, display_mode, ui_scale: Option<..> }` (resource, `ctx.window()`): `new()`, `with_resolution(w, h)`, `with_display_mode(DisplayMode)`, `has_changes()`, `clear()`. Prefer the `ctx.request_*` helpers.
- `WindowInfo { scale_factor: f64, display_mode: DisplayMode }` (resource kept current by the engine).
- `CursorRequest { cursor, custom_cursor, visible, confined, locked }` (resource, `ctx.cursor()`): `has_changes()`. Prefer the `ctx.set_cursor*` helpers.
- `enum CursorStyle { Default, Text, Crosshair, Pointer, ResizeHorizontal, ResizeVertical, ResizeNeSw, ResizeNwSe, Move, NotAllowed, Grab, Grabbing, Wait, Progress, Help, Cell, ZoomIn, ZoomOut }`.

---

## Post-processing

`EffectsManager` is a resource (`ctx.effects()`); timed effects run on real time.

| | |
|---|---|
| `set_fade(progress)`, `set_fade_color(progress, Color)` | 0 visible, 1 faded |
| `set_color_tint(Color, intensity)`, `set_sepia(intensity)` | grading |
| `set_vignette(intensity)`, `set_vignette_custom(intensity, radius, softness)` | |
| `flash(Color, Duration)`, `flash_secs(Color, f32)`, `flash_white(Duration)`, `flash_red(Duration)` | fading screen flash |
| `enable_crt()`, `enable_crt_custom(scanlines, curvature, rgb_offset)`, `disable_crt()` | |
| `set_chromatic_aberration(offset)`, `disable_chromatic_aberration()` | |
| `set_bloom(intensity)`, `set_bloom_custom(threshold, intensity, radius, blur_passes)`, `disable_bloom()`, `has_bloom()` | |
| `set_god_rays(intensity, light_positions: Vec<(f32, f32)>)`, `set_god_rays_custom(intensity, samples, decay, density, threshold, positions)`, `update_god_rays_lights(positions)`, `disable_god_rays()`, `has_god_rays()` | |
| `set_ambient_occlusion(intensity, radius)`, `set_ambient_occlusion_custom(.., bias)`, `disable_ambient_occlusion()`, `has_ambient_occlusion()` | |
| `add_custom(name, ShaderHandle) -> Result<(), EffectError>`, `add_custom_in(name, ShaderHandle, EffectStage) -> Result<(), EffectError>`, `set_custom_param(name, param, value) -> bool`, `set_custom_enabled(name, bool) -> bool`, `remove_custom(name) -> bool`, `has_custom(name)` | game-written full-screen WGSL; `add_custom` runs on the display image, `add_custom_in` in the stage you pick |
| `new()`, `update(dt)`, `time()`, `active_effects()`, `active_effects_in(EffectStage)`, `has_active_effects()`, `has_active_effects_in(EffectStage)`, `clear()` | |

`enum PostProcessEffect { Fade, ColorTint, Vignette, ScreenFlash, Crt, ChromaticAberration, GodRays, AmbientOcclusion, Bloom }` (what `active_effects` yields); `PostProcessEffect::stage() -> EffectStage`; `enum EffectError { DuplicateName(String) }`.

`enum EffectStage { Hdr, Display }`: `Hdr` runs on the scene image in linear light that may exceed 1, before the look (bloom, god rays, ambient occlusion); `Display` runs on the display image after it (fade, tint, vignette, flash, CRT, chromatic aberration). A custom effect's `scene(uv)` is linear in both; `Color` params stay sRGB bytes/255, so decode one (`srgb_to_linear`) before mixing it with `scene(uv)`.

Bloom's `threshold` is linear luminance, 0 or more and unbounded above (default 1.0, white): only light brighter than white blooms, so lit highlights, emission and intensity above 1 feed it. God rays' threshold is the same measure.

---

## Trails and lines

- `with_emission(f32)` (default 1) on `Trail2D`, `Line2D`, `ParticleEmitter` and `Renderable` multiplies the colour's light: above 1 it glows past white and feeds bloom. `Trail2D` and `Line2D` read it with `emission()`; `Renderable` and `ParticleEmitter` have a public `emission` field.
- `Trail2D` (component, on an entity with a `Transform`): records the entity's world position every fixed step (points are world space). Constructors `new(Color, width)`, `bullet(Color)`, `swipe(Color, w)`, `particle(Color)`, `laser(Color, w)`, `fading_laser(Color, w, fade_time)`, `lightning(Color, w)`, `fire(Color, w)`, `energy(Color, w)`; builders `with_length(max_points)` (≤ 128), `with_end_width`, `with_taper(ratio)`, `with_fade(bool)`, `with_end_color`, `with_min_distance`, `with_emit_interval`, `with_lifetime(secs)`, `with_fade_distance(px)`, `with_texture(SpriteHandle)`, `with_texture_mode(TextureMode)`, `with_tile_length`, `with_style(TrailStyle)`, `with_effect(TrailEffect)`, `with_cap(LineCap)`, `with_join(LineJoin)`, `with_blend(BlendMode)`, `with_filter(Filter)` (textured trails), `with_lighting(bool)`, `with_z_order(i32)`; control `set_enabled`, `set_visible`, `set_recording`, `clear`, `push_point(x, y)`, `push_point_back`, `populate_line(..)`, `update(x, y, dt)`, plus getters/setters for each setting.
- `Line2D` (component, on an entity with a `Transform`): drawn at the entity's world position (points are offsets from it). `new(points: Vec<Vec2>, Color, width)`, `segment(start, end, Color, w)`, `horizontal(y, x0, x1, Color, w)`, `vertical(x, y0, y1, Color, w)`, `rect(x, y, w, h, Color, line_w)`, `circle(center, r, Color, w, segments)`, `arc(..)`, `quadratic_bezier(..)`, `bezier(..)`, `arrow(start, end, Color, w, head)`; builders `with_end_color`, `with_end_width`, `with_taper`, `with_closed(bool)`, `with_z_order`, `with_style`, `with_cap`, `with_join(LineJoin)`, `with_blend(BlendMode)`, `with_lighting(bool)`; editing `points_mut`, `set_points`, `push_point`, `pop_point`, `clear`; `length()`, `point_at_distance(d)`, `color_at(t)`, `width_at(t)`.
- `TrailPoint { x, y, age }`: `new(x, y)`.
- `enum TrailStyle { Solid, Soft, Glow }`, `enum LineCap { Butt, Round, Square }`, `enum LineJoin { Miter (default), Bevel, Round }`, `enum TextureMode { Stretch, Tile }`, `enum TrailEffect { Lightning { .. }, Noise { .. }, UvScroll { .. }, Ribbon { .. } }`.
- The engine draws both through the 2D batch as joined polylines (no vertex limit; translucent corners are no darker than their segments).

## Debug drawing

`ctx.debug_draw() -> Mut<'_, DebugDraw>` (or `ResMut<DebugDraw>` in a system): world-space shapes drawn unlit on top of the world, one screen pixel wide in every camera at any zoom.

| `DebugDraw` / `DebugPen` | |
|---|---|
| `line(a: Vec2, b: Vec2, Color)`, `polyline(&[Vec2], Color)` | lines |
| `rect(center: Vec2, size: Vec2, Color)`, `circle(center: Vec2, radius, Color)` | outlines |
| `arrow(from: Vec2, to: Vec2, Color)`, `cross(at: Vec2, size, Color)` | markers (world units; the arrow's head is a few pixels) |
| `width(pixels) -> DebugPen`, `layers(RenderLayers) -> DebugPen` | a pen with the same methods, chainable: `debug.width(3.0).layers(RenderLayers::layer(1)).circle(..)` |

Shapes added in a fixed step (`update`, fixed systems) are drawn until the next step begins; shapes added elsewhere for one frame. The engine's collider, velocity and origin overlays use it.

---

## Skeletal animation

| Type | API |
|---|---|
| `SkeletonDef { bones: Vec<BoneDef>, bone_indices }` | `SkeletonDef::builder().bone(name, parent: Option<&str>, position: Vec2, rotation, length).build()` |
| `BoneDef { name, parent: Option<usize>, rest_transform: BoneTransform, length }` | |
| `BoneTransform { position: Vec2, rotation, scale: Vec2 }` | `IDENTITY`, `new(position, rotation)`, `rotation(r)`, `position(p)`, `lerp(&other, t)` |
| `Pose { transforms: Vec<BoneTransform> }` | `from_rest(&SkeletonDef)`, `lerp(&other, t)` |
| `Skeleton` (component) `{ def: Arc<SkeletonDef>, pose: Pose, world_transforms: Vec<BoneWorldTransform> }` | `new(Arc<SkeletonDef>)`, `update_world_transforms(pos, rot)` |
| `BoneWorldTransform { position: Vec2, rotation }` | |
| `SkeletalAnimation { name, tracks, duration, loop_mode }` | `SkeletalAnimation::builder(name, duration, Arc<SkeletonDef>).loop_mode(LoopMode).track(bone, \|t\| t.at(time, BoneTransform, Ease)).build()`, `sample(time, &base_pose) -> Pose` |
| `SkeletalAnimator` (component) `{ animation, time, speed, playing, just_finished }` | `new(Arc<SkeletalAnimation>)`, `play`, `pause`, `reset`, `update(dt)`, `sample(&base) -> Pose` |
| `SkeletalController` (component) | `new(Arc<SkeletalAnimation>, id, Arc<SkeletonDef>)`, `with_animation(anim, id)`, `with_transition(from, to)`, `with_bidirectional`, `with_transition_from_any`, `with_blend_duration(from, to, secs)`, `with_default_blend_duration`, `set_state(id) -> bool`, `set_state_then(id, next) -> bool`, `force_state`, `current()`, `is_blending()`, `blend_progress()`, `update(dt)`, `sample() -> Pose` |
| `SkeletalBlender` | `new()`, `start_blend(from: Pose, duration)`, `update(dt)`, `apply(&target) -> Pose`, `is_blending()`, `blend_progress()` |
| `draw_skeleton(&mut DebugDraw, def: &SkeletonDef, world: &[BoneWorldTransform], bone_color, joint_color, width_px, joint_radius)` | debug drawing (call every update) |

The engine advances each `Skeleton`'s controller (or standalone animator) every fixed step and places the bones at the entity's `GlobalTransform`.

---

## Particles

The scene owns a `ParticleSystem`; emitters and attractors are components.

| `ParticleSystem` | |
|---|---|
| `new()`, `with_seed(u64)` | |
| `update(&mut self, world: &mut World, dt: f32)` | call in `update` |
| `collect_for_render(&mut self, world: &World, collector: &mut BatchCollector)` | call in `render`: `collect_for_render(gpu.world, gpu.batch)`; particles are centred on their position, in world space |
| `particle_count()`, `clear()`, `clear_emitter(Entity)` | |

- `ParticleEmitter` (component; emits from its entity's `GlobalTransform`) with public fields for every setting (`mode`, `shape`, `sprite`, `enabled`, lifetime/size/color/speed/rotation ranges, `gravity`, `drag`, `z_order`, `effect`, `blend`, `filter`, `lighting`, `collision`, `layer`, `max_particles`). `new(SpriteHandle)`, `with_mode(EmissionMode)`, `with_shape(EmissionShape)`, `with_lifetime(min, max)`, `with_size(min, max)`, `with_end_size(min, max)`, `with_color(start, end)`, `with_speed(min, max)`, `with_rotation(min, max)`, `with_angular_velocity(min, max)`, `with_gravity(x, y)`, `with_drag(f32)`, `with_z_order(i32)`, `with_effect(SpriteEffect)`, `with_blend(BlendMode)`, `with_filter(Filter)`, `with_lighting(bool)`, `with_layer(u32)`, `with_collision(CollisionConfig)`, `with_max_particles(u32)`, `one_shot()`, `set_enabled(bool)`, `reset()`, `burst(count)`, `is_finished()`. Presets (each `(SpriteHandle) -> Self`): `fire`, `flame_small`, `smoke`, `smoke_thick`, `magic`, `magic_heal`, `rain`, `snow`, `sparks`, `dust`, `confetti`, `bubbles`, `comet_trail`, `exhaust`; `explosion(sprite, count)`, `explosion_debris(sprite, count)`.
- `enum EmissionMode { Continuous { rate }, Burst { count }, BurstRepeating { count, interval, repeats: Option<u32> } }`.
- `enum EmissionShape { Point, Line { start_x, start_y, end_x, end_y }, Circle { radius, hollow }, Rectangle { width, height }, Cone { direction, spread, min_speed, max_speed } }` (radians).
- `ParticleAttractor { attractor_type, layer_mask, enabled }` (component): `new(AttractorType)`, `point(strength, radius)`, `gravity_well(strength, radius)`, `line(x0, y0, x1, y1, strength, radius)`, `area(w, h, fx, fy)`, `vortex(strength, radius)`, `spiral(strength, radius, inward_pull)`, `turbulence(strength, scale, speed)`, `with_layer_mask(u32)`, `with_enabled(bool)`. `enum AttractorType { Point { strength, radius, falloff }, Line { .. }, Area { .. }, Vortex { .. }, Turbulence { .. } }`.
- `CollisionConfig { radius, collision_mask, response, send_events }`: `new(radius)`, `with_mask(u32)`, `with_response(CollisionResponse)`, `with_events()`, `die_on_hit(r)`, `bouncy(r, restitution)`, `sticky(r)`. `enum CollisionResponse { Die, Bounce { .. }, Stick, Event }` (`bounce(restitution)`, `restitution()`). Particles collide with `Collider` entities.
- `ParticleCollision { emitter_entity, collider_entity, position_x, position_y, normal_x, normal_y, velocity_x, velocity_y }` is a message (`ctx.messages::<ParticleCollision>()`); a bounce writes one only above `MIN_BOUNCE_EVENT_SPEED` (20 px/s).

See `ryo-particles`.

---

## Tilemaps

Tile data, coordinates, collision and Tiled import; the renderer does not draw tilemaps yet.

- `TilemapManager` (resource, `ctx.tilemaps()`), keyed by the tilemap's entity (a record is dropped when its entity is despawned): `register(Entity, Tilemap)`, `unregister(Entity)`, `get_tile(e, layer, x, y) -> Option<TileId>`, `set_tile(e, layer, x, y, Option<TileId>)`, `world_to_tile(e, wx, wy) -> Option<(u32, u32)>`, `tile_to_world(e, tx, ty)` (top-left), `is_solid(e, wx, wy)`, `get_tilemap(e)`, `visible_chunks(e, min_x, min_y, max_x, max_y) -> Vec<ChunkCoord>`.
- `Tilemap { tilesets, layers, tile_width, tile_height, map_width, map_height, orientation }`: `new(tile_w, tile_h, map_w, map_h)`, `add_layer(TilemapLayer) -> usize`, `add_tileset(Tileset) -> usize`, `pixel_width()`, `pixel_height()`, `world_to_tile(x, y)`, `tile_to_world(tx, ty)`, `tile_to_world_center(tx, ty)`, `tile_at_world(layer, x, y)`, `is_solid_at(x, y)`. `enum TilemapOrientation { Orthogonal, Isometric, HexagonalFlat, HexagonalPointy }` (coordinate conversion is orthogonal only).
- `TilemapLayer { name, tiles: Vec<Option<TileId>>, width, height, tileset_index, visible, opacity, z_order, parallax_x, parallax_y }`: `new(name, w, h)`, `in_bounds(x, y)`, `get_tile(x, y)`, `set_tile(x, y, TileId)`.
- `Tileset { name, image_path, tile_width, tile_height, columns, tile_count, spacing, margin, image_width, image_height, properties }`: `tile_uv(id)`, `get_properties(id) -> TileProperties`. `TileProperties { solid, platform, animation: Option<TileAnimation>, custom }`, `TileAnimation { frames: Vec<(TileId, u32 /* ms */)> }`, `type TileId = u32`.
- Auto-tiling: `AutoTileSet::new(name, terrain_id: u16, AutoTileMode, default_tile)`, `add_rule(bitmask: u8, TileId)`, `resolve(bitmask) -> TileId`; `enum AutoTileMode { Minimal, Full }`; `compute_bitmask_minimal(x, y, w, h, is_same: &dyn Fn(u32, u32) -> bool) -> u8`, `compute_bitmask_full(..)`.
- Collision: `generate_collision_rects(&Tilemap) -> Vec<CollisionRect>`, `generate_collision_rects_region(&Tilemap, x0, y0, x1, y1)` (greedy-merged solid tiles); `CollisionRect { x, y, w, h }` (tiles), `to_world(tile_w, tile_h) -> (f32, f32, f32, f32)`.
- Chunks: `ChunkCoord { chunk_x, chunk_y, layer_index }`, `ChunkState { coord, dirty, has_tiles }` (16×16 tiles).
- Tiled: `load_tiled_map(path: &str) -> Result<(Tilemap, Vec<TiledObject>), TilemapError>`; `TiledObject { name, obj_type, x, y, width, height, rotation (deg), shape: TiledShape, properties }`; `enum TiledShape { Rect, Ellipse, Point, Polygon(Vec<(f32, f32)>), Polyline(..) }`; `enum TiledValue { Bool, Int, Float, String }`; `enum TilemapError { Io, Parse, MissingTileset, UnsupportedFeature }`.

---

## Render pipeline

`RenderPipeline` (resource, `ctx.render_pipeline()`): the frame's passes, in order. It belongs to the engine, so changes persist across scenes. Camera passes (`PassScope::Camera`) run first, once per camera in render order, each on that camera's image; then the frame passes run once.

| `RenderPipeline` | |
|---|---|
| `push(impl RenderPass) -> Result<(), RenderPipelineError>` | append |
| `insert_before(anchor: &str, pass)`, `insert_after(anchor, pass)` | relative to a pass id |
| `remove(id) -> Option<Box<dyn RenderPass>>` | |
| `set_enabled(id, bool) -> Result<(), RenderPipelineError>`, `is_enabled(id) -> Option<bool>`, `contains(id)`, `pass_ids()`, `passes_to_run(&FrameInfo)` (in run order: camera passes, then frame passes) | |
| `new()`, `default_2d()` | |

`pass_ids`: per camera `CLEAR`, `WORLD` (entities, sprites, lit trails and lines in z order), `LIGHTING`, `UNLIT_WORLD`; per frame `CAMERAS` (window cameras' images into their viewports), `HDR_EFFECTS` (bloom, god rays, ambient occlusion on the scene image), `TONEMAP` (the look: the scene image into the display image), `TRANSITION`, `POST_PROCESS` (display effects), `UI`, `COMPOSITE`, `CAPTURE`. `enum RenderPipelineError { DuplicatePass(String), UnknownPass(String) }`.

```rust
pub trait RenderPass: Send + Sync {
  fn id(&self) -> &str;
  fn should_run(&self, frame: &FrameInfo) -> bool { true }
  fn scope(&self) -> PassScope { PassScope::Frame } // or PassScope::Camera
  fn execute(&mut self, ctx: &mut RenderContext<'_>);
}
```

- `RenderContext`: `device()`, `queue()`, `encoder()`, `frame() -> &FrameInfo`, `render_size()` (the scene image in pixels), `screen_size() -> Vec2`, `format()` (of `current_view()`: `Rgba16Float` before `tonemap` and in camera passes, `Rgba8UnormSrgb` after it), `current_view()` (in a camera pass, the camera's image), `encoder_and_current_view()`, `camera() -> Option<&CameraView>` and `camera_entity() -> Option<Entity>` (`Some` in camera passes). GPU types come from `ryo2d::wgpu`.
- `FrameInfo { screen_width, screen_height, render_width, render_height, world, lighting, hdr_effects, effects, ui, transition, capture }`: what the frame contains (`hdr_effects`: HDR-stage effects are active; `effects`: display-stage ones).

See `ryo-renderer`.

---

## Config and saves

Two persistence tiers: player settings in `system.json` (`SystemState`) and game data in slots (`GameSaveData<T>`).

| `SaveManager` | |
|---|---|
| `new(save_dir) -> SaveResult<Self>`; usually `Workspace::discover()?.create_save_manager()?` | |
| `save_system(&SystemState)`, `load_system() -> SaveResult<SystemState>`, `load_or_create_system(&SystemDefaults)`, `load_or_create_system_filtered(&SystemDefaults, &UserPreferenceFilter)`, `has_system_save()` | settings |
| `save_game<T: Serialize>(slot, &T)`, `save_game_with_info(slot, GameSaveData<&T>)`, `load_game<T: DeserializeOwned>(slot) -> SaveResult<GameSaveData<T>>`, `has_game_save(slot)`, `delete_game(slot)`, `list_game_saves() -> SaveResult<Vec<String>>` | compressed `.ryo-save` slots |
| `save_raw(filename, &T)`, `load_raw(filename)` | arbitrary files |
| `save_dir()`, `set_save_dir(path)`, `set_compression_level(0..9)` | |

- `SystemState { master_volume, music_volume, sfx_volume, width, height, display_mode, vsync: Option<bool>, show_fps: Option<bool>, env }`: `new()`, `from_defaults(&SystemDefaults)`, `merge_defaults(..)`, `set_env(key, value)`, `get_env`, `get_env_str`, `get_env_bool`, `get_env_i64`, `get_env_f64`.
- `GameSaveData<T> { data, timestamp, name, play_time, metadata }`: `new(data)`, `with_name`, `with_play_time(secs)`, `with_metadata(k, v)`.
- `enum UserPreference { MasterVolume, MusicVolume, SfxVolume, Resolution, DisplayMode, Vsync, ShowFps }` (`all()`); `UserPreferenceFilter`: `all()`, `none()`, `only(iter)`, `contains`, `is_empty`, `len`.
- `enum DisplayMode { Windowed, ExclusiveFullscreen, BorderlessFullscreen }` (`all()`, `display_name()`); `Resolution { width, height }` (`new`, `common()`, `display_name()`); `parse_resolution("1280x720") -> Option<(u32, u32)>`; `compress_data(&[u8])`, `decompress_data(&[u8])`.
- `Workspace` (`Ryo.toml` project): `discover()`, `discover_from(dir)`, `from_path(root)`, `root()`, `name()`, `config() -> &WorkspaceConfig`, `entry_point()`, `assets_dir()`, `save_location()`, `save_data_dir()`, `set_save_data_path(p)`, `create_save_manager()`, `env_str/env_bool/env_int/env_float(key)`, `has_env(key)`, `has_config_file()`.
- `WorkspaceConfig { name, title, description, version, entry_point, save_data, assets, debug, system: SystemDefaults, release, env }`; `SystemDefaults { resolution, vsync, display_mode, master_volume, music_volume, sfx_volume, show_fps, dev_console, target_background_fps, background_rendering, env }`.

```rust
let saves = Workspace::discover()?.create_save_manager()?;
saves.save_game("slot1", &Progress { level: 3 })?;
let loaded: GameSaveData<Progress> = saves.load_game("slot1")?;
```

See `ryo-config-save`.

---

## UI

Immediate-mode UI drawn in `Scene::ui` with `UiContext`; retained state structs hold widget state between frames. Full guide: `ryo-ui`.

| `UiContext` | |
|---|---|
| `button(text, Rect) -> bool`, `button_styled(text, Rect, normal, hover, pressed, text_color) -> bool`, `label(text, x, y, Color, bg: Option<Color>)` | |
| `slider(Rect, &mut f32, min, max) -> bool`, `checkbox(Rect, &mut bool, label: Option<&str>) -> bool`, `toggle(Rect, &mut bool) -> bool` | value widgets |
| `text_input(&mut TextInputState, Rect, placeholder, dt) -> bool`, `dropdown(&mut DropdownState, Rect, &[&str]) -> bool`, `scroll_view(&mut ScrollViewState, Rect, content_height, dt, \|ui, scroll_y\| ..) -> bool`, `scroll_view_horizontal(..)` | stateful widgets |
| `draw_text(text, x, y, Color)`, `draw_text_anchored(text, Anchor, ox, oy, Color)`, `draw_text_centered(text, Rect, Color)`, `draw_text_wrapped(..)`, `draw_text_wrapped_centered(..)`, `text_size(text)`, `wrap_text(..)` | text (bitmap font) |
| `draw_rect(Rect, Color)`, `draw_rect_outline(Rect, Color, thickness)`, `draw_panel(Rect, bg, border: Option<Color>)`, `draw_themed_panel(Rect)`, `draw_progress_bar(Rect, progress, fill, bg)`, `draw_progress_bar_with_border(..)`, `draw_themed_progress_bar(Rect, progress)`, `draw_sprite(&Sprite, x, y)`, `draw_sprite_scaled(..)`, `draw_sprite_handle(&SpriteHandle, x, y)`, `draw_sprite_handle_scaled(..)`, `draw_modal_overlay(Color)`, `draw_focus_ring(..)`, `draw_focus_ring_themed(Rect)` | drawing |
| `is_clicked(Rect)`, `is_hovered(Rect)`, `is_held(Rect)` | hit tests |
| `input() -> &UiInput`, `consume_key(KeyCode)`, `consume_click()`, `consume_escape()`, `consume_scroll()`, `click_consumed()`, `escape_consumed()`, `wants_keyboard()`, `set_mouse_occluded()` | input; consumption carries over to gameplay |
| `push_clip(Rect)`, `pop_clip()`, `with_no_clip(f)`, `current_clip()`, `has_clip()`, `clip_depth()`, `clear_clips()`, `point_in_clip(x, y)` | clipping |
| `handle_tab_navigation(&mut [&mut dyn Focusable])`, `handle_dropdown_tab_navigation(..)`, `finish_dropdowns()`, `any_dropdown_open()`, `focused_dropdown()`, `set_focused_dropdown(id)` | focus |
| `dt()` (real time), `screen_size()`, `ui_scale()`, `theme()` | |

- `Color { r, g, b, a: u8 }`: `new(r, g, b, a)`, `rgb(r, g, b)`, `hex(0xRRGGBB | 0xRRGGBBAA)`, `with_alpha(a)`, `to_array()`, `to_normalized()`, `to_linear()` (sRGB decoded to linear light); constants `TRANSPARENT`, `BLACK`, `WHITE`, `RED`, `GREEN`, `BLUE`, `YELLOW`, `CYAN`, `MAGENTA`, `GRAY`, `DARK_GRAY`, `LIGHT_GRAY`, `HEALTH_RED`, `MANA_BLUE`, `STAMINA_GREEN`, `XP_GOLD`, `PANEL_BG`, `PANEL_BORDER`.
- `Rect { x, y, width, height }`: `new(x, y, w, h)`, `from_pos_size`, `centered_at(cx, cy, w, h)`, `right()`, `bottom()`, `center()`, `contains(x, y)`, `shrink(p)`, `expand(p)`.
- Layout: `VBox::new(x, y, spacing)` / `HBox::new(x, y, spacing)` with `next(w, h) -> Rect`, `with_halign(HAlign)` / `with_valign(VAlign)`, `bounds()`, `reset()`; `Stack::new(x, y, w, h)` / `from_rect` with `centered(w, h)`, `aligned(w, h, HAlign, VAlign)`, `padded(p)`, `split_horizontal(w)`, `split_vertical(h)`. `enum HAlign { Left, Center, Right }`, `enum VAlign { Top, Center, Bottom }`.
- Retained state: `TextInputState` (`new()`, `with_text`, `text()`, `set_text`, `is_focused`, `set_focused`, selection and clipboard methods), `DropdownState` (`new()`, `with_selected(i)`, `with_id(u32)`, `selected()`, `set_selected`, `is_open`, `open`, `close`, `toggle`), `ScrollViewState` (`new()`, `scroll_y()`, `set_scroll_y`, `scroll_to_top`, `scroll_to_bottom(content_h, view_h)`), `FocusTrap::new(&mut [&mut dyn Focusable])` (`focus_next`, `focus_prev`, ...), `trait Focusable { fn is_focused(&self) -> bool; fn set_focused(&mut self, bool); .. }`.
- `UiInput`: the frame's input as the UI sees it (`mouse_position`, `mouse_pressed`, `key_pressed`, `key_repeat`, `text_input`, `gamepad_button_pressed`, `left_stick`, `consume_key`, `consume_mouse`, `consume_scroll`, ...).
- UI tweens (driven by the owner with `update(ui.dt())`): `UiTween<T: Lerp>` with `new(start, end, duration)`, `with_ease(Ease)`, `with_delay`, `with_loop(UiTweenLoop)`, `on_start`, `on_complete`, `update(dt)`, `value()`, `progress()`, `is_finished()`, `is_playing()`, `restart`, `pause`, `resume`, `stop`, `retarget(target)`; aliases `TweenFloat`, `TweenVec2` (`(f32, f32)`), `TweenColor`, `TweenRect`; `enum UiTweenLoop { Once, Loop, PingPong, Times(u32) }`.
- Themes: `Theme` (`new()`, `dark()`, `light()`, `load(path)`, `save(path)`), `HotReloadTheme` (`new(path)`, `poll_reload()`, `force_reload()`, `get()`, `set(Theme)`, `path()`), `ThemeError`.
- Fonts: `BitmapFont` (`load(path, char_w, char_h)`, `from_rgba(..)`, `with_spacing`, `with_char_range`, `text_width`, ...), `create_debug_font() -> BitmapFont` (the built-in 8×8 font).
- `Anchor` is the same type as the sprite anchor.

---

## Errors

| Type | Variants | Alias |
|---|---|---|
| `EngineError` | `Window(String)`, `Render(String)`, `Save(SaveError)`, `WorkspaceError(String)`, `Gpu(GpuError)` | `EngineResult<T>` |
| `GpuError` | `NoAdapter`, `RequestDevice`, `CreateSurface`, `DeviceLost` (`String`) | |
| `AssetError` | `NotFound(PathBuf)`, `LoadError { path, reason }`, `WatchError(String)`, `Sprite(SpriteError)` | `AssetResult<T>` |
| `SpriteError` | `DecodeError { path, reason }` | `SpriteResult<T>` |
| `AudioError` | `InitError`, `LoadError`, `PlayError`, `NotFound` (all `String`), `Unavailable` | `AudioResult<T>` |
| `HotReloadError` | `WatcherCreation(String)` | `HotReloadResult<T>` |
| `SaveError` | `Serialization`, `Deserialization`, `Compression`, `Decompression` (`String`), `Io(io::Error)`, `NotFound(PathBuf)`, `InvalidSlot(String)` | `SaveResult<T>` |
| `NetworkError` | `InitError`, `ConnectionError`, `SendError`, `ReceiveError`, `TransportError` (`String`) | `NetworkResult<T>` |
| `ReplicationError` | `SerializationError`, `DeserializationError` (`String`), `TypeMismatch { expected, found }`, `UnknownComponent(ComponentTypeId)`, `EntityNotFound(u64)` | `ReplicationResult<T>` |

All are `thiserror` enums (`Display`, `Error`); `SaveError` and `GpuError` convert into `EngineError` with `?`.

---

## Navigation

Grid A* plus steering; the game moves its agents.

- `NavigationGrid`: `new(world_w, world_h, cell_size)`, `with_origin(w, h, cell, ox, oy)`, `dimensions()`, `cell_size()`, `generation()`, `world_to_grid(x, y) -> GridCoord`, `grid_to_world(GridCoord)` (cell centre), `grid_to_world_corner(..)`, `in_bounds`, `get`/`set(GridCoord, CellState)`, `is_walkable`, `cost`, `neighbors`, `line_of_sight(a, b)`, `clear()`, `block_rect(min_x, min_y, max_x, max_y)`, `block_collider(&GlobalTransform, &Collider, Entity)`, `rebuild_from_world(&mut World)`.
- `NavigationBuilder::new(world_w, world_h).with_cell_size(f32).with_origin(x, y)`, then `build()` or `build_from_world(&mut World)` (blocks every collider's cells).
- `enum CellState { Walkable, Blocked, Difficult(f32) }` (`cost()`, `is_walkable()`); `GridCoord { x: i32, y: i32 }` (`new`).
- `Pathfinder`: `new()`, `with_config(PathfinderConfig)`, `config()`, `find_path(&self, &grid, sx, sy, gx, gy) -> PathResult`, `find_path_grid(&self, &grid, GridCoord, GridCoord)`, `start_pathfinding(&mut self, &grid, sx, sy, gx, gy) -> (u64, PathResult)`, `continue_pathfinding(&grid, id)`, `cancel_pathfinding(id)`, `active_request_count()`.
- `PathfinderConfig { max_nodes_per_frame, max_total_nodes, heuristic_weight, default_smoothing, tie_breaking }`: `with_time_slicing(n)`, `with_smoothing(PathSmoothing)`, `with_heuristic_weight(w)`.
- `enum PathResult { Found(Path), InProgress, NoPath, InvalidEndpoints, TooExpensive }`: `is_found()`, `path() -> Option<Path>`, `is_in_progress()`.
- `Path`: `new(waypoints, generation)`, `empty()`, `is_valid()`, `is_complete()`, `len()`, `waypoints()`, `current_waypoint()`, `destination()`, `advance()`, `reset()`, `total_length()`, `remaining_length()`, `is_stale(grid_generation)`, `smooth_line_of_sight(&grid)`, `smooth_spline(segments)`. `PathNode { x, y, grid_coord }`. `enum PathSmoothing { None, LineOfSight, Spline }`.
- `NavAgent` (component) `{ current_path, speed, arrival_threshold, avoid_agents, radius, path_smoothing }`: `new(speed)`, `with_arrival_threshold`, `with_avoidance(radius)`, `with_smoothing`, `state() -> NavAgentState`, `set_path(Path)`, `start_computing()`, `arrive()`, `fail()`, `stop()`, `update(dt)`, `has_path()`, `destination()`, `remaining_distance()`, `just_entered_state()`, `time_in_state()`. `enum NavAgentState { Idle, Computing, Moving, Arrived, Failed }`.
- `NavTarget` (component) `{ x, y, dynamic_target, follow_entity, repath_threshold }`: `position(x, y)`, `follow(Entity)`, `with_dynamic(bool)`, `with_repath_threshold(f32)`, `update_position`, `needs_repath`.
- `steering` module (`Vec2` in, velocity out): `seek(pos, target, max_speed)`, `flee(..)`, `arrive(pos, target, max_speed, slow_radius)`, `separation(pos, radius, neighbors: &[(Vec2, f32)], strength)`, `cohesion(..)`, `alignment(neighbors: &[(Vec2, Vec2)], max_speed)`.

---

## State machines

- `StateMachine<S>`: `new(initial)`, `with_transition(from, to)`, `with_transitions_from(from, iter)`, `with_transition_from_any(to)`, `with_bidirectional(a, b)`, `with_history(capacity)`, `add_transition`, `remove_transition`, `transition_to(to) -> TransitionResult<S>`, `force_transition(to)`, `set_state(s)`, `current()`, `previous()`, `is(&s)`, `can_transition_to(&s)`, `available_transitions()`, `time_in_state()`, `total_time()`, `update(dt)`, `history()`, `clear_history()`, `reset(initial)`; fields `just_entered`, `just_exited`.
- `StateStack<S>`: `new(root)`, `with_max_depth(n)`, `push(s) -> bool`, `push_if_not_current(s)`, `pop() -> Option<S>`, `pop_to(&s)`, `pop_to_root()`, `replace(s)`, `reset(root)`, `current()`, `current_mut()`, `root()`, `previous()`, `is`, `contains`, `depth()`, `is_at_root()`, `time_in_state()`, `update(dt)`, `iter()`, `as_slice()`; fields `just_pushed`, `just_popped`.
- `enum TransitionError<S> { NotAllowed { from, to }, AlreadyInState(S) }`; `type TransitionResult<S> = Result<S, TransitionError<S>>`.

---

## Networking

Client-server over renet (UDP; WebRTC with the `matchbox` feature). The game polls its server or client each frame and forwards events itself. Full guide: `ryo-network`.

| Item | |
|---|---|
| `NetworkServer::new(NetworkServerConfig) -> NetworkResult<Self>` | `update(Duration)`, `events()`, `send(ClientId, Channel, &[u8])`, `broadcast(Channel, &[u8])`, `broadcast_except(..)`, `receive(ClientId, Channel) -> Option<Vec<u8>>`, `clients()`, `disconnect(id)`, `is_running()`, `connected_count()` |
| `NetworkClient::connect(NetworkClientConfig) -> NetworkResult<Self>` | `update(Duration)`, `events()`, `send(Channel, &[u8])`, `receive(Channel)`, `is_connected()`, `is_connecting()`, `is_disconnected()`, `disconnect()`, `disconnect_reason()`, `client_id()` |
| `ListenServer::new(NetworkServerConfig)` | host plays too: `host_id()` (= `LOCAL_HOST_ID`), `remote_clients()`, `all_clients()`, `is_host(id)`, `remote_count()`, `player_count()`, plus the server methods |
| `NetworkServerConfig { port: 5000, max_clients: 16, protocol_id: 0, transport }` | `new()` / `Default` |
| `NetworkClientConfig { server_addr, protocol_id, transport }` | `udp(SocketAddr, protocol_id)`, `webrtc(WebRtcConfig)` |
| `enum TransportConfig { Udp, WebRtc(WebRtcConfig) }` | `WebRtcConfig`, `IceServer` need `matchbox` |
| `enum NetworkEvent { ClientConnected { .. }, ClientDisconnected { .. }, Connected, Disconnected { .. } }` | |
| `enum Channel { Unreliable, ReliableOrdered, ReliableUnordered, ReplicationReliable, ReplicationUnreliable }` | the last two are reserved for replication |
| `ClientId` (`u64`), `LOCAL_HOST_ID: ClientId = 0`, `trait ServerLike`, `trait ClientLike` | abstractions over server and client types |

Families (see `ryo-network`):

- Replication: `ReplicationManager`, `NetworkEntity`, `NetworkEntityBuilder`, `NetworkId`, `Authority`, `trait Replicated`, `ComponentRegistry`, `ComponentTypeId`, `SerializedComponent`, `DirtyTracker`, `NetworkedValue<T>`, messages `ReplicationMessage`, `SpawnEntityMessage`, `DespawnEntityMessage`, `ComponentUpdateMessage`, `ComponentBatchMessage`, `FullStateSyncMessage`.
- RPC: `trait Rpc`, `RpcRegistry`, `RpcTarget`, `RpcTypeId`, `ReceivedRpc<T>`, `SerializedRpc`.
- Input with prediction: `#[derive(NetworkInput)]`, `trait NetworkInputTrait`, `SequencedInput<T>`, `InputBuffer<T>`, `trait NetworkedInputHandler`, `InputProcessor<H>`.
- Actions: `trait NetworkAction`, `ActionContext`, `ActionError`, `SerializedAction`, `execute_action`, `try_execute_action`, `execute_action_server`, `execute_action_broadcast`, `apply_received_action`.
- Prediction: `Predicted` (component), `PredictionHelper`, `ServerPositionUpdate`, `UpdateResult`.

---

## Profiling, console, logging

- `profile_function!()` and `profile_scope!("name")`: puffin scopes with the `profiler` feature, nothing without. See `ryo-profiler`.
- `game_trace!`, `game_debug!`, `game_info!`, `game_warn!`, `game_error!`: `format!`-style game logging (`game_info!("score {}", s)`; `game_info!(module: "ai", "..")` tags a module), shown in the logger window and the terminal.
- Console (backtick key): `ConsoleCommandBuilder` (from `EngineBuilder::with_console_commands`): `declare(name, description)`, `declare_verbose(name, description, verbose)`, `remove_builtin(name)`. Handlers: `ctx.console().register(name, |args: &[&str], cmd: &mut CommandContext| -> CommandResult)`. `CommandContext { console: &mut ConsoleState, debug_config, quit_requested, capture_requested, time_scale, commands, game_name, game_version }`; `ConsoleState` prints with `print`, `print_colored`, `print_error`, `print_system`; `enum CommandResult { Ok, Error(String), Quit }`; `CommandRegistry` is the engine's registry. See `ryo-console`.
- `rand` and `rand::Rng` are re-exported (`rand::thread_rng().gen_range(0.0..1.0)`).

---

## Optional crates

Procedural generation and inventory live outside the core prelude:

```toml
[dependencies]
ryo2d = { path = "../ryo2d" }
ryo2d-procgen = { path = "../ryo2d/crates/ryo2d-procgen" }     # dungeons, caves, noise, WFC
ryo2d-inventory = { path = "../ryo2d/crates/ryo2d-inventory" } # items, inventories, equipment, loot
```

```rust
use ryo2d::prelude::*;
use ryo2d_procgen::prelude::*;   // ProcgenBuilder, GenerationGrid, TileType, Region, ...
use ryo2d_inventory::prelude::*; // ItemRegistry, Inventory, Equipment, LootTable, ...
```

See the `ryo-procgen` skill and `crates/ryo2d-procgen/AGENTS.md`, `crates/ryo2d-inventory/AGENTS.md`.

---

## Testing (`ryo2d::testing`)

| Item | Signature / purpose |
|------|---------------------|
| `HeadlessEngine::new(builder: EngineBuilder)` | `-> Result<HeadlessEngine, HeadlessError>`: the engine without a window |
| `HeadlessEngine::new_or_skip(builder)` | `-> Option<HeadlessEngine>`: `None` when no GPU (tests return early) |
| `HeadlessEngine::with_scale_factor(builder, f64)`, `new_or_skip_with_scale_factor(builder, f64)` | a HiDPI window (physical pixels per logical pixel); the default is 1.0 |
| `step()` / `step_frames(n)` | `-> Result<StepOutcome, HeadlessError>`: each step advances one fixed timestep (one `FixedUpdate`) and renders |
| `frame()` | `-> Result<RgbaImage, HeadlessError>`: the last scene image (scene + UI, before the bars): the render size |
| `window_frame()` | `-> Result<RgbaImage, HeadlessError>`: the last window, bars included |
| `frame_count()`, `resize(w, h)` | `resize` as if the window became `w`x`h` physical pixels |
| `world()` / `world_mut()` | the world between steps |
| `press_key` / `release_key` / `tap_key` / `type_text(&str)` | keyboard input (`KeyCode`) |
| `press_mouse` / `release_mouse` / `mouse_move(x, y)` | mouse input (screen space) |
| `mouse_move_window(x, y)` | the pointer at physical window pixels, mapped like a real cursor |
| `StepOutcome` | `Running` \| `Quit` |
| `HeadlessError` | `NoGpu` \| `Render(String)` \| `Capture(CaptureError)` |
| `shared_gpu()` | one GPU device shared across tests |
