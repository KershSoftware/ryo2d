---
name: ryo-api
description: Complete API reference for ryo2d::prelude - every exported type with its signatures and use, organised by area.
metadata:
  version: 2.28.0
---

# Ryo2D API Reference

Everything `use ryo2d::prelude::*` brings in, with signatures taken from the code. Other skills explain how to use an area; this one is the lookup. When a signature here and the code disagree, the code (and its rustdoc) wins.

Conventions: 2D with +Y down, units are pixels, rotation is radians unless a name says degrees, `Color` is sRGB `u8`. The engine has three clocks and runs gameplay at a fixed rate; see [Time](#time) and [Phases](#phases-and-sets).

```rust
use ryo2d::prelude::*;
```

Contents: [ECS kernel](#ecs-kernel) · [Transform and hierarchy](#transform-and-hierarchy) · [Engine components](#engine-components) · [Juice](#juice-tweens-knockback-squash) · [Pooling](#pooling) · [Time](#time) · [Phases](#phases-and-sets) · [Scenes](#scenes) · [SceneContext](#scenecontext) · [EngineBuilder](#enginebuilder) · [Messages](#messages) · [Input](#input) · [Audio](#audio) · [Cameras](#cameras) · [Images](#images) · [Sprite animation](#sprite-animation) · [GPU sprites and parallax](#gpu-sprites-and-parallax) · [Composite tiles](#composite-tiles) · [Physics](#physics) · [Characters and gameplay](#characters-and-gameplay) · [Lighting](#lighting) · [Assets](#assets) · [Window and cursor](#window-and-cursor) · [Post-processing](#post-processing) · [Trails and lines](#trails-and-lines) · [Debug drawing](#debug-drawing) · [Skeletons](#skeletal-animation) · [Particles](#particles) · [Tilemaps](#tilemaps) · [Render pipeline](#render-pipeline) · [Config, settings and saves](#config-settings-and-saves) · [Text](#text) · [UI](#ui) · [Errors](#errors) · [Navigation](#navigation) · [State machines](#state-machines) · [Networking](#networking) · [Profiling, console, logging](#profiling-console-logging) · [Optional crates](#optional-crates) · [Testing](#testing-ryo2dtesting)

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
| `Add`, `Insert`, `Remove` | A component's lifecycle, for observers: `On<Add<T>>` as an entity gains a `T`, `On<Insert<T>>` each time one is put on (added or put over), `On<Remove<T>>` as it loses it (despawned too); `add.entity` is the entity |
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

<!-- given: world: &mut World, image: Handle<Image> -->
<!-- with: #[derive(Component)] struct Player { speed: f32 } -->
```rust
let e = world.spawn((Transform::from_xy(100.0, 50.0), Renderable::image(image))).id();
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

```rust,ignore
// Definition
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

`GlobalTransform` (world space, written only by the engine): `IDENTITY`, `from_transform(&Transform)`, `translation() -> Vec2`, `rotation() -> f32` (the turn of its x axis), `scale() -> Vec2` (axis lengths, never negative), `is_mirrored() -> bool` (a negative scale on one axis, on the entity or above it; both axes negative is a half turn, not a mirror), `affine() -> Affine2`, `transform_point(Vec2) -> Vec2`, `to_transform() -> Transform` (a mirror kept as a negative height), `mul_transform(&Transform) -> Self` (a child's global), `lerp(&Self, t) -> Self` (keeps a mirror; between a mirrored transform and one that is not, each axis goes straight to where it ends).

- Inserting a `Transform` (spawn or `insert`) sets `GlobalTransform` at once (under the parent's current global for a child). Later edits through `get_mut::<Transform>` reach `GlobalTransform` at propagation: the end of each fixed step (`FixedSet::Transforms`, after physics) and in `PostUpdate`.
- **A negative scale mirrors.** A `Transform` scaled by -1 on one axis mirrors what is under it, and what draws by a `GlobalTransform` draws mirrored: a sprite (across its x axis, with its anchor's offset; a tiled run and a composite tile are not mirrored), a skeleton, world text, a tile layer, a collider. So a sprite on a mirrored parent, or on a flipped skeleton's socket, mirrors with it. A light occluder is placed by its entity's translation and turn alone, and is not mirrored.
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

An image, a tiled run of one, or the entity's skeleton, on an entity with a `Transform`.

```rust,ignore
// Fields
pub struct Renderable {
  pub image: Handle<Image>, pub style: RenderStyle, pub visible: bool, pub z_order: i32,
  pub flip_x: bool, pub flip_y: bool, pub alpha: f32, pub tint: Color,
  pub shader: Option<SpriteShader>, pub drop_shadow: Option<DropShadow>,
  pub blob_shadow: Option<BlobShadow>, pub contact_shadow: Option<ContactShadow>,
  pub anchor: Anchor, // default Anchor::Center
  pub target: Option<RenderTarget>, // drawn instead of `image` (see render_target)
  pub blend: BlendMode,  // default Alpha (Premultiplied for render_target)
  pub filter: Filter,    // default Filter::Default (the presentation's)
  pub lighting: bool,    // default true; false draws it (and its shadows) after lighting
  pub emission: f32,     // default 1; above 1 it glows past white and feeds bloom
}
pub enum RenderStyle { Single, Tiled { direction: TileDirection, target_size: f32 }, Skeleton }
```

| Method | |
|---|---|
| `image(Handle<Image>) -> Self` | a single sprite, placed by its anchor; it holds the image. An image that is not loaded draws the checker in debug builds and nothing in release ones |
| `render_target(RenderTarget) -> Self` | shows a render target at its size, placed, tinted, faded, scaled, turned and flipped like a sprite (see [Cameras](#cameras)); blends `Premultiplied` |
| `tiled(Handle<Image>, TileDirection, target_size: f32) -> Self` | repeated to fill `target_size`, laid out from the entity's position (first tile's top-left); ignores `anchor` |
| `skeleton() -> Self` | draws the entity's `Skeleton` (see [Skeletons](#skeletal-animation)): its regions where its bones put them, sorted, lit, lifted and shadowed as a sprite is. `z_order`, `visible`, `lighting` and `filter` are the whole skeleton's; `alpha`, `tint`, `blend`, `emission` and the sprite `shader` apply to each region; `flip_x` and `flip_y` mirror it about the entity's position, bones and sockets with it; the drop shadow is cast part by part (darker where parts overlap); `image`, `anchor` and `target` are not used. Its bones are drawn between fixed steps as the entity is, which `Skeleton` makes `Interpolated`; remove `Interpolated` for step-rate drawing |
| `with_anchor(Anchor)`, `with_z_order(i32)`, `with_visible(bool)`, `with_flip_x(bool)`, `with_flip_y(bool)`, `with_alpha(f32)`, `with_tint(Color)` | builders |
| `with_shader(SpriteShader)`, `with_drop_shadow(DropShadow)`, `with_blob_shadow(BlobShadow)`, `with_contact_shadow(ContactShadow)` | effects |
| `with_blend(BlendMode)`, `with_filter(Filter)`, `with_lighting(bool)` | how it is drawn (shadows always blend `Alpha`) |
| `with_emission(f32)` | multiplies the sprite's light (default 1). Above 1 it glows past white and feeds bloom; a lit sprite is still darkened by lighting, so to glow in the dark add `with_lighting(false)`. |

Blend and filter modes (on `Renderable`, `ParticleEffect`, `SpriteDraw`, `ParallaxLayer`, `Trail2D`, `Line2D`):

| `BlendMode` | Result |
|---|---|
| `Alpha` (default) | ordinary transparency |
| `Premultiplied` | the texture's colour is already multiplied by alpha (render targets) |
| `Additive` | adds light: fire, sparks, glows, lasers |
| `Multiply` | darkens what is below by its colour |
| `Screen` | lightens what is below without blowing out; a 0..1 formula, so over pixels brighter than white (HDR light, emission) it dims them, and an emissive Screen drawable over a bright pixel can go below zero and show black: use `Additive` for light above white |

`enum Filter { Default, Nearest, Linear }`: `Default` is nearest under `pixel_art` and linear otherwise. Linear sampling uses mipmaps, so sprites drawn small stay smooth; nearest keeps hard texels.
| `has_shader`, `has_drop_shadow`, `has_blob_shadow`, `has_contact_shadow`, `is_single`, `is_tiled`, `is_skeleton`, `tile_count(&Assets<Image>) -> u32` | queries |

Shadows (all `Default`, builder style):

- `DropShadow { offset: Vec2, color, blur, scale: Vec2 }`: `new()`, `offset(x, y)`, `color(c)`, `blur(f32)`, `scale(x, y)`, `below(offset_y)`, `diagonal(offset)`, `squashed(offset_y, squash)`.
- `BlobShadow { radius_x, radius_y, offset_y, color, height, max_height, height_scaling }`: `new()`, `circle(r)`, `ellipse(rx, ry)`, `radii(x, y)`, `offset(y)`, `color(c)`, `with_height_scale(bool)`, `max_height(f32)`, `set_height(&mut, f32)`, `effective_radius()`, `effective_alpha()`.
- `ContactShadow { radius, intensity, offset_y, color, falloff }`: `new()`, `radius(f32)`, `intensity(f32)`, `offset(y)`, `color(c)`, `falloff(f32)`, `sharp(r)`, `soft(r)`, `alpha_at_distance(d)`.

### Velocity, ZLayer, YSort

- `Velocity { pub dx: f32, pub dy: f32 }` (pixels per second): `new(dx, dy)`, `zero()`, `from_vec2(Vec2)`, `as_vec2()`. Physics integrates it for `RigidBody` entities, and sweeps a character (`CharacterController`) by it, leaving the real velocity; nothing integrates it otherwise.
- `ZLayer { Background = -1000, Default = 0, Foreground = 1000, UI = 2000 }`: `as_z_order(self) -> i32`.
- `YSort { foot_offset, sort_offset, depth_scale_enabled, scale_min, scale_max, y_min, y_max }`: sort y-sorted entities by their feet, wherever the anchor is. `new()` (feet at the bottom, `foot_offset` 1.0), `with_foot_offset(0..1)`, `with_sort_offset(px)`, `with_depth_scale(scale_min, scale_max, y_min, y_max)`, `without_depth_scale()`, `calculate_sort_y(top_y, sprite_height)`, `calculate_depth_scale(y) -> (f32, f32)`. Order is by `z_order` first; within one `z_order`, plain sprites draw first, then y-sorted ones by sort y. A skeleton has no image to measure down: it sorts by its entity's y plus `sort_offset`, and `foot_offset` plays no part.

### Material, SpriteShader, SpriteEffect

`Material { pub shader: SpriteShader }` is a component; it wins over `Renderable.shader`.

| | Constructors (same set on `Material` and `SpriteShader`) |
|---|---|
| built-in | `outline(Color, thickness: u32)`, `white_outline(u32)`, `glow(Color, radius)`, `flash(Color, intensity)`, `damage_flash(intensity)`, `color_shift(Color, amount)`, `silhouette(Color)`, `shadow()`, `pixelate(block: u32)`, `wave(amplitude, frequency, speed)`, `dissolve(progress)`, `dissolve_with_edge(progress, Color)`, `invert()`, `grayscale(intensity)`, `directional_outline(solid: Color, passable: Color, u32)`, `platform_outline(u32)` |
| custom WGSL | `custom(Handle<Shader>)`, `custom_with_params(Handle<Shader>, ShaderParams)`, `.with_param(name, impl Into<ParamValue>)` |
| other | `Material::new(SpriteShader)`, `is_builtin()`, `is_custom()`, `update(&mut self, dt) -> bool` |

- `enum SpriteShader { BuiltIn(SpriteEffect), Custom { shader: Handle<Shader>, params: ShaderParams } }` (the sprite holds its shader); also `as_builtin()`, `shader_handle() -> Option<&Handle<Shader>>`, `params()`, `params_mut()`.
- `enum SpriteEffect { Outline, Glow, Flash, ColorShift, Silhouette, Pixelate, Wave, Dissolve, Invert, Grayscale, DirectionalOutline }` with the constructors above plus `glow_with_intensity`, `hit_flash`, `full_grayscale`; `is_animated()`, `update(dt) -> bool` (advances a `Wave`'s phase; the engine does not call it), `apply_cpu(..)`.
- `Shader` (an asset): `from_wgsl(name, source)` (`name` is what its errors are reported under), `name()`, `source()`. Load one from a `.wgsl` file with `ctx.assets().load_now::<Shader>(path)` (named by its asset path) or embed one with `ctx.assets().add(Shader::from_wgsl(..))`; a reload or a `get_mut` compiles it again (a shader already drawn keeps drawing while its new version compiles off the main thread, then draws it from the start of a frame), and shaders with equal sources compile once. `ShaderLoader` compiles the file as it loads (as a sprite shader if it defines `fn sprite`, an effect if `fn effect`), so WGSL that does not compile is `AssetError::Decode` at its line and column, and a broken save is `ReloadFailed`, keeping the last good shader. A version that fails in the renderer anyway (made in code, or a pipeline the GPU rejects) draws the last version the GPU accepted, warned once per version.
- `ShaderParams`: `new()`, `set(name, value) -> &mut Self`, `with(name, value) -> Self`, `get(name) -> Option<ParamValue>`. `enum ParamValue { F32, Vec2, Vec3, Vec4, Color }`, `From<f32 | Vec2 | Vec3 | Vec4 | Color>` (a `Color` is sRGB bytes divided by 255, not linear).
- A custom sprite shader's time input is game time (it stops with pause and slows with speed).

`trait Lerp: Clone + Send + Sync + 'static { fn lerp(start: &Self, end: &Self, t: f32) -> Self; }`, implemented for `f32`, `(f32, f32)`, `Vec2`, `Color`, `Rect`.

---

## Juice: tweens, knockback, squash

Tweens and squash and stretch run in `FixedSet::Juice` (before `Scene::update`), knockback in `FixedSet::Movement` (after it, before physics); all on the fixed step, reporting through [messages](#messages).

### Tween (component, Transform plus Renderable or Text2d alpha/tint)

```rust,ignore
// Fields
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

- The alpha target (`to_alpha`, `fade_to`, `fade_out`, `fade_in`) fades a `Renderable`'s alpha, or a `Text2d`'s `opacity` (not part of its layout, so a fade never lays text out again). The tint target applies to `Renderable` only.
- `enum TweenLoop { Once, Loop, PingPong, Times(u32) }`.
- `enum Ease { Linear, In/Out/InOut × Quad, Cubic, Quart, Sine, Expo, Back, Elastic, Bounce }` (e.g. `Ease::OutBack`); `apply(&self, t) -> f32`.
- `TweenSequence { pub playing, pub just_finished, pub loop_sequence, pub speed }`: `new()`, `then(Tween)`, `then_all(iter)`, `with_loop()`, `with_speed(f32)`, `set_speed`, `current()`, `current_mut()`, `current_index()`, `len()`, `is_empty()`, `is_finished()`, `pause`, `resume`, `stop`, `restart`, `advance`. Engine-driven.
- `PropertyTween<T: Lerp>` (component): `new(start, end, duration)`, `with_easing`, `with_delay`, `with_loop(TweenLoop)`, `with_speed`, `on_start`, `on_update`, `on_complete`, `value() -> T`, `progress()`, `playing()`, `is_finished()`, `just_started()`, `just_finished()`, `pause`, `resume`, `stop`, `restart`, `retarget(new_end)`, `update(dt)`. The engine drives `PropertyTween<f32>`, `<Vec2>` and `<Color>`; call `update(dt)` yourself for other types.

<!-- given: player: Entity -->
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
| `push(&mut self, direction, force)`, `push_with_decay(..)`, `stop` (also drops a hit not yet given to a dynamic body), `restart`, `update(dt)` | on a component value |
| `current_velocity() -> (f32, f32)`, `strength()`, `force()`, `direction() -> Vec2`, `decay_rate()`, `is_finished()` | state |

A character (`CharacterController`) is swept by the push, so surfaces stop it and the part running into them is dropped. A dynamic body takes each hit as one change in velocity of its own (the same speed whatever its mass), and physics slows it from there. A kinematic body without a controller and a fixed body ignore it (logged once). An entity without a body is moved through its `Transform`. The system is `update_knockback` in `FixedSet::Movement` (`register_gameplay_systems`): a push applied in `update` acts in that step's physics, and `KnockbackStarted` / `KnockbackFinished` reach the next step's `update`.

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

```rust,ignore
// Signatures
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
- A `RecycleOldest` pool puts its oldest entity back (disabled) before handing it out again (enabled), so whatever watches `Disabled` sees a release and an acquire: physics and joints register it again, and an `AudioEmitter` on it stops its loops and plays again.

---

## Time

`Time` is a resource and `ctx.time`:

```rust,ignore
// Fields
pub struct Time { pub real: RealClock, pub game: GameClock, pub fixed: FixedClock, /* .. */ }
```

| Clock | Methods | Use |
|---|---|---|
| `real: RealClock` | `delta() -> f32`, `delta_duration()`, `elapsed() -> f32` | wall clock; UI animation, camera shake, timed post effects |
| `game: GameClock` | `delta()`, `delta_duration()`, `elapsed()`, `speed()`, `set_speed(f32)` (clamped to `0..=GameClock::MAX_SPEED`, 100), `pause()`, `resume()`, `is_paused()`, `hitstop(secs)`, `is_hitstop()`, `clear_hitstop()`, `max_delta()`, `set_max_delta(Duration)` (default 250 ms; never below one fixed timestep) | game time: real delta times speed, capped at max_delta per frame, zero while paused or in hitstop |
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
| `PreUpdate` | a running scene transition, then the asset pump (loads and reloads land, unheld assets are freed, `AssetEvent`s sent), then tile maps (those whose assets are ready start landing, landings that came back land with `MapEvent::Spawned`, spawned maps follow saved files with `MapEvent::Reloaded`, streamed maps unload (keeping the chunks a player changed, despawning their `ChunkOwned` entities) and land chunks round their anchors with `ChunkEvent`s, class tiles follow the chunks); then the console |
| UI pass | `Scene::ui` for visible scenes, bottom-up; input it consumes never reaches gameplay |
| `FixedUpdate` × 0..N | `FixedSet::Snapshot` → `Juice` → `Scene` (`Scene::update`) → `Game` (scene and game systems) → `Movement` (climbers changing storey on tile maps' stairs, before the kill zones' respawns; trigger behaviours, tile maps' collision kept in step with their tiles, tile collider rebuilds, movement input, knockback, movement presets) → `Physics` (one system: the whole physics step) → `Transforms` → `Skeletons` (skeletons advance, pose their bones, place their sockets and send their events) → `Particles` (emitters spawn, particles move and die) → `Trails` |
| `Update` | `UpdateSet::Game` (scene per-frame systems) |
| `PostUpdate` | tile maps' shadows kept in step with their tiles, then `PostUpdateSet::Transforms`, then a skeleton added or reloaded this frame gets its pose → `Camera` (shake, then every camera's `CameraView`) → `Extract` (unscaled particle emitters step here) |
| render | draw list from the world, `Scene::render`, the engine's particles, render pipeline |
| `Last` | tracker clearing |

- `enum FixedSet { Snapshot, Juice, Scene, Game, Movement, Physics, Transforms, Skeletons, Particles, Trails }`, `enum UpdateSet { Game }`, `enum PostUpdateSet { Transforms, Camera, Extract }`: order your systems with `.in_set(..)`, `.before(..)`, `.after(..)`.
- Game-wide systems: `EngineBuilder::add_systems(phase, systems)` (any phase; in `FixedUpdate`/`Update`, a call whose systems are not ordered against anything outside it runs in its `Game` set; one that names a set or another call's system (`.in_set`, `.before`, `.after`) keeps exactly that order). Scene-owned systems: `ctx.add_systems(Phase::FixedUpdate | Phase::Update, systems)`, dropped when the scene leaves the stack.
- Every schedule is single-threaded.

---

## Scenes

```rust,ignore
// Signatures, with their defaults
pub trait Scene: 'static {
  fn on_enter(&mut self, ctx: &mut SceneContext) {}
  fn on_exit(&mut self, ctx: &mut SceneContext) {}
  fn on_pause(&mut self, ctx: &mut SceneContext) {}                       // another scene pushed on top
  fn on_resume(&mut self, ctx: &mut SceneContext, returned: Option<Box<dyn Any>>) {}
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction { SceneAction::Continue } // each fixed step
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction { SceneAction::Continue } // once a frame, before the steps
  fn render(&mut self, gpu: &mut GpuRenderContext) {}                    // GPU-batched sprites, parallax
  fn is_transparent(&self) -> bool { false }  // scenes below are drawn and get `ui`
  fn updates_below(&self) -> bool { false }   // scenes below keep updating and running their systems
  fn is_physics_paused(&self) -> bool { false }
  fn holds_audio_below(&self) -> bool { true } // the scenes it does not update are not heard (their voices held)
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

```rust,ignore
// Fields
pub struct SceneContext<'a> {
  pub world: &'a mut World,
  pub time: &'a mut Time,
  pub input: &'a mut Input,
  /* the scene's schedules, message cursors, console scope */
}
```

`time` and `input` are taken out of the world for the callback: use the fields (`ctx.world.resource::<Time>()` panics inside a callback). Systems always see them as resources. Cameras are entities: `ctx.camera()` reaches the main one.

| Helper | |
|---|---|
| `assets(&mut self) -> AssetsView<'_>` | the asset registry: load, add and read assets (see [Assets](#assets)); it borrows the world, so take it, use it and let it go |
| `audio(&mut self) -> AudioView<'_>` | the audio engine: play sounds and music, snapshots, bus volumes (see [Audio](#audio)); it borrows the world like `assets()` |
| `saves(&self) -> SavesView<'_>` | the game's saves: save, load, list and delete slots (see [Config, settings and saves](#config-settings-and-saves)); it borrows the context, so take it for each use |
| `settings(&mut self) -> Mut<'_, Settings>` | the player's settings: volumes, window size, display mode, vsync, the FPS counter, shadow quality and the game's sections; a change applies that frame and saves itself |
| `particles(&mut self) -> ParticlesView<'_>` | the engine's particles: play an effect once, read the live count against the budget, clear them (see [Particles](#particles)); it borrows the world like `assets()` |
| `maps(&mut self) -> MapsView<'_>` | spawned tile maps: read and edit tiles, Wang terrain, cell conversions, world levels (see [Tilemaps](#tilemaps)); it borrows the world like `assets()` |
| `add_systems<M>(&mut self, phase: Phase, systems: impl IntoScheduleConfigs<ScheduleSystem, M>) -> Result<(), SceneScheduleError>` | scene-owned systems, run in `FixedSet::Game` / `UpdateSet::Game`; add them once, in `on_enter`. Refused, adding nothing: other phases (`SceneScheduleError::UnsupportedPhase`), a place they never run, such as `.after(FixedSet::Physics)` (`EngineSet`, naming the set), and the same line adding the same systems in a later callback (`AlreadyAdded`, with its location). `SceneScheduleError` is in the prelude |
| `messages<T: Message + Clone>(&mut self) -> Vec<T>` | messages written since this scene last read `T` |
| `write_message<T: Message>(&mut self, message: T)` | registers `T` on first use |
| `resource<T: Resource>(&self) -> Option<&T>` | |
| `resource_mut<T>(&mut self) -> Option<Mut<'_, T>>` | |
| `resource_or_default<T: Default>(&mut self) -> Mut<'_, T>` | |
| `insert_resource<T: Resource>(&mut self, value: T)` | |
| `effects(&mut self) -> Mut<'_, EffectsManager>` | post-processing |
| `lighting(&mut self) -> Mut<'_, LightingSettings>` | ambient, atmosphere, forced on/off |
| `look(&mut self) -> Mut<'_, Look>` | exposure, tonemap curve, colour grading (see [The look](#the-look)) |
| `render_pipeline(&mut self) -> Mut<'_, RenderPipeline>` | toggle or insert passes (persist across scenes) |
| `fonts(&mut self) -> Mut<'_, Fonts>` | add fonts, pick render modes and role fonts (see [Text](#text)) |
| `debug_draw(&mut self) -> Mut<'_, DebugDraw>` | debug lines and shapes over the world (see [Debug drawing](#debug-drawing)) |
| `camera(&mut self) -> CameraMut<'_>` | the main camera (respawned over the design area if the game despawned it) |
| `render_targets(&mut self) -> Mut<'_, RenderTargets>` | textures cameras render into and sprites show |
| `actions(&mut self) -> &mut Actions` | bind, rebind and reset the game's actions (read them with `ctx.input.action_pressed(..)`; see [Input](#input)) |
| `ui_theme(&mut self) -> Mut<'_, UiTheme>` | the UI theme (`get`, `set`, `modify`), from the next frame (see [UI](#ui)) |
| `follow_ui_theme(&mut self, theme: Handle<Theme>)` | follow a theme asset: used at once if loaded (else when it lands), and again whenever it changes; a `modify` after it lasts until the asset next changes |
| `spawn_ui(&mut self, tree: UiNode) -> Entity` | spawn a UI node tree as a root this scene owns (despawned when the scene leaves the stack); see [Node trees](#node-trees) |
| `spawn_ui_in(&mut self, parent: Entity, tree: UiNode) -> Option<Entity>` | add a subtree as the last child of a container node; `None` (warned) if `parent` is not one |
| `find_ui(&self, name: &str) -> Option<Entity>`, `find_ui_in(&self, root: Entity, name: &str) -> Option<Entity>` | the first node with that `.name(..)` in this scene's trees (spawn order), or in one tree |
| `ui_id(&self, node: Entity) -> Option<UiId>`, `focus_ui(&mut self, node: Entity)` | a node's id once drawn; focus a node when it is next drawn |
| `ui_events(&mut self) -> Vec<UiEvent>` | this scene's nodes' events since it last read them; shares `messages::<UiEvent>()`'s cursor |
| `spawn_ui_file(&mut self, path: &str) -> Result<Entity, UiFileError>` | load a UI file, build its `root` and spawn it (see [UI files](#ui-files)) |
| `ui_template(&mut self, path: &str, name: &str, args: &UiArgs) -> Result<UiNode, UiFileError>` | load a UI file and build an instance of its template `name`, to spawn or add to a tree |
| `update_ui(&mut self, node: Entity, tree: UiNode) -> Entity` | bring a spawned tree in line with `tree` in place; returns the node, or its replacement when `tree`'s top is a different kind. A node that is not from a tree warns once and is returned unchanged |
| `ui_set(&mut self, node: Entity, name: &str, value: impl Into<BindValue>)`, `ui_get<T: TryFrom<BindValue>>(&self, node: Entity, name: &str) -> Option<T>` | a named binding of the tree `node` belongs to, or of its list row (each row of a list is its own scope); `T` is `f32`, `f64`, `i32`, `u32`, `usize`, `bool` or `String` |
| `ui_list_count(&mut self, list: Entity, count: usize)` | a list node's item count (state: a reload or `update_ui` writes it only when the tree's count changed); a focus on an item past the new end moves to the new last item |
| `ui_list_refresh(&mut self, list: Entity)`, `ui_list_refresh_row(&mut self, list: Entity, index: usize)` | send `UiListBind` again next frame for every pooled row, or for item `index`'s row if pooled; the rows keep drawing until bound |
| `ui_list_scroll_to(&mut self, list: Entity, index: usize)`, `ui_list_focus(&mut self, list: Entity, index: usize)` | scroll a list node by the least that shows item `index`'s row (a jump further than a view lands one view short, then eases); `ui_list_focus` also focuses the row's first focusable node, up to two frames after it is bound |
| `ui_list_index(&self, node: Entity) -> Option<usize>` | the item of the list row `node` is in (itself or the nearest row above it), to map a `UiEvent` to its item. Each `ui_list_*` call on a node that is not a list warns once and does nothing |
| `window(&mut self) -> Mut<'_, WindowRequest>`, `cursor(&mut self) -> Mut<'_, CursorRequest>` | the UI scale and cursor requests, applied after the frame (the window's size and display mode are `settings()`) |
| `console(&mut self) -> ConsoleScope<'_>` | scene-scoped console handlers |
| `spawn_child(&mut self, parent: Entity, bundle: impl Bundle) -> Entity` | |
| `skip_interpolation(&mut self, entity: Entity)` | |
| `screen_size() -> (f32, f32)`, `screen_width()`, `screen_height()` | screen space: the window in logical pixels, or the pixel-art target |
| `scale_factor() -> f64`, `display_mode() -> DisplayMode` | from `WindowInfo` |
| `request_ui_scale(Option<f32>)` | the UI scale, render pixels per UI unit (`None`: automatic), applied after the frame |
| `set_cursor(CursorStyle)`, `set_cursor_visible(bool)`, `show_cursor()`, `hide_cursor()`, `set_cursor_confined(bool)` (not on macOS or the web: the cursor stays free, warned once), `cursor_confinement_supported() -> bool`, `set_cursor_locked(bool)`, `set_custom_cursor(&Handle<Image>, hot_x: u16, hot_y: u16)`, `set_custom_cursor_centered(&Handle<Image>)` (an image not loaded leaves the cursor as it is) | cursor |

Helpers that return `Mut<..>` borrow the context: chain one call (`ctx.effects().set_bloom(0.6)`) or bind `let mut fx = ctx.effects();` for several.

`ConsoleScope`: `command::<S>(spec: impl Into<CommandSpec>, Fn(&mut S, &mut SceneContext, &mut Command) -> Result<(), String>)` (removed when the scene leaves the stack), `remove(name) -> bool`, `print(text)`, `print_colored(text, color)`.

A complete scene:

<!-- with: #[derive(Component)] struct Player { speed: f32 } fn move_players(time: Res<Time>, mut players: Query<(&Player, &mut Transform)>) {} -->
```rust
#[derive(Default)]
struct Level { player: Option<Entity>, sparks: Option<Handle<ParticleEffect>> }

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let sprite = ctx.assets().add(Image::solid_rect(16, 16, 255, 255, 255, 255));
    self.sparks = Some(ctx.assets().add(ParticleEffect::sparks(sprite.clone())));
    let player = ctx.world.spawn((
      Transform::from_xy(320.0, 180.0),
      Renderable::image(sprite).with_z_order(10),
      Player { speed: 120.0 },
      RigidBody::dynamic(),
      Collider::rect(16.0, 16.0),
      Velocity::zero(),
      Interpolated,
    )).id();
    self.player = Some(player);
    ctx.add_systems(Phase::FixedUpdate, move_players).expect("FixedUpdate takes scene systems");
    ctx.lighting().set_ambient(AmbientLight::night());
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    for hit in ctx.messages::<CollisionStarted>() {
      if let Some(player) = self.player && hit.involves(player) {
        Knockback::apply(ctx.world, player, Vec2::new(-1.0, 0.0), 300.0);
        ctx.time.game.hitstop(0.05);
        ctx.camera().add_trauma(0.4);
        let at = ctx.world.get::<Transform>(player).map(|t| t.translation);
        if let (Some(sparks), Some(at)) = (&self.sparks, at) {
          ctx.particles().play_at(sparks, at);
        }
      }
    }
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if ui.button("Quit").at(Rect::new(8.0, 8.0, 80.0, 20.0)).clicked() {
      return SceneAction::Quit;
    }
    SceneAction::Continue
  }
}
```

---

## EngineBuilder

<!-- with: #[derive(Resource, Default)] struct Score(u32); #[derive(Message, Clone)] struct CoinCollected { value: u32 } #[derive(Component)] struct Player { speed: f32 } fn move_players(time: Res<Time>, mut players: Query<(&Player, &mut Transform)>) {} #[derive(Default)] struct Level; impl Scene for Level {} -->
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
| `shadow_quality(ShadowQuality)` | the game's default shadow quality (default `ShadowQuality::High`; over Ryo.toml's `[system] shadow_quality`); the player's saved choice wins when kept |
| `background_fps(u32)`, `background_rendering(bool)` | unfocused behaviour |
| `pixel_art(w: u32, h: u32)`, `window_size(w: u32, h: u32)`, `resizable(bool)`, `display_mode(DisplayMode)`, `ui_scale(Option<f32>)`, `ui_resolution(UiResolution)` | presentation; `ui_scale(None)` (default) keeps UI units equal to screen units, `Some(s)` is `s` render pixels per UI unit; `UiResolution::Render` (default) or `Window` |
| `ui_theme(Theme)` | the starting UI theme (default `Theme::dark()`) |
| `particle_budget(u32)` | the most particles alive at once, over every emitter (default `DEFAULT_PARTICLE_BUDGET`, 20,000); a spawn past it is dropped, with one warning per effect |
| `asset_loader(impl AssetLoader)` | a loader for the game's own asset type, registering the type too; for an extension an engine loader of the same type takes, the game's wins |
| `asset_source(impl AssetSource)` | read assets from this source instead of the project's `assets/` folder (a test's `MemorySource`) |
| `tiled_project(&str)` | the Tiled project (an asset path) whose class and enum types Tiled maps use; over `[tilemap] tiled_project`, default `project.tiled-project` at the asset root |
| `hot_reload(bool)` | whether a saved asset file reloads while the game runs; default on in debug builds with a window, off in release and headless ones |
| `ui_cache(bool)` | whether node trees replay their unchanged nodes instead of building them (default `true`); `false` builds every node every frame, the same UI slower, to rule caching out of a UI bug (also `DebugConfig::ui_cache`, the console's `ui:cache on\|off`) |
| `ui_fragments(bool)` | whether node trees keep the layout and resolved output of their unchanged subtrees and copy them (default `true`; only while `ui_cache` is on); `false` replays and resolves every unchanged subtree every frame, the same UI slower, to rule the copies out of a UI bug (also `DebugConfig::ui_fragments`, the console's `ui:fragments on\|off`) |
| `ui_retain(bool)` | whether the renderer keeps the UI image while the UI is unchanged and reuses unchanged texts' glyphs (default `true`); `false` draws the whole UI every frame, the same pixels slower, to rule retention out of a rendering bug (also `DebugConfig::ui_retain`, the console's `ui:retain on\|off`) |
| `show_colliders(bool)`, `show_fps(bool)`, `show_velocities(bool)`, `show_origins(bool)` | debug overlays |
| `with_user_preferences(impl Into<UserPreferenceFilter>)` | which player settings `system.json` keeps (the others last for the session); repeated calls combine |
| `save_versions<T: 'static>(SaveVersions<T>)` | a saved type's current version and its upgrade steps; a gap panics here, at the call |
| `game_settings::<T>(key: &str)`, `game_settings_versioned::<T>(key: &str, SaveVersions<T>)` | the game's own settings section `key` (`T: Default + Serialize + DeserializeOwned + Send + Sync + 'static`), kept in `system.json` |
| `memory_saves(MemorySaves)` | keep the saves and `system.json` in this in-memory store, never on disk (tests; a headless engine has its own anyway) |
| `project(ProjectFile)` | the game's Ryo.toml compiled in with `ryo2d::project!()`, for builds with no Ryo.toml to read (the web) |
| `insert_resource(impl Resource)` | resource for every scene and system |
| `add_systems<M>(Phase, impl IntoScheduleConfigs<ScheduleSystem, M>)` | game-wide systems |
| `add_message::<T: Message>()` | register a message type up front |
| `console_command(spec, Fn(&mut World, &mut Command) -> Result<(), String>)` / `remove_console_command(name)` | add a console command for the run / remove one |
| `start_scene(impl Scene + 'static)` | first scene |
| `build(self) -> Engine`, then `Engine::run(self) -> EngineResult<()>` | |

Settings resolve in order: engine defaults, `Ryo.toml` (or values embedded by `ryo2d build`, or the `project` file), builder calls, then the player's `system.json` values for the settings `with_user_preferences` keeps.

The resolution model: `resolution(w, h)` (or `[system] resolution` in `Ryo.toml`) fixes screen space at `w`x`h` units: the UI, the mouse and `ctx.screen_size()` use it, and the main camera's `Projection::Fit` shows exactly that world area. The frame renders at the window's physical resolution, scaled by any factor to fit the window and centred with bars where the aspect ratios differ (`Presentation::Fit`), so it fills a full-screen window and stays sharp. `pixel_art(w, h)` instead renders into a `w`x`h` target scaled by a whole number, centred with bars, nearest sampling, sprites snapped to whole target pixels. With neither, the game is resolution-independent: the world renders at the window's physical size, the main camera keeps 1280x720 world units in view (a wider window shows more world), and screen space is the window in logical pixels. `window_size(w, h)` sets the window's starting logical size (by default the resolution, shrunk to fit 1280x720 when larger; for pixel art the largest whole multiple that fits 1280x720).

`enum Presentation { ResolutionIndependent (default), Fit { width, height }, PixelArt { width, height } }`. `ScreenMapping` converts between window pixels and screen space: `new(presentation, physical, scale_factor)`, `screen_size()`, `scale()`, `window_rect()`, `window_to_screen(Vec2)`, `screen_to_window(Vec2)`, `render_size()`, `ndc_rect()`, `presentation()`.

---

## Messages

Engine messages (all `Message + Clone`):

| Type | Fields | Written when |
|---|---|---|
| `CollisionStarted` / `CollisionStopped` | `a, b` (colliders), `body_a, body_b`, and on started `contact: Option<Contact { point, normal }>`; `involves(e) -> bool`, `other(e)`, `other_collider(e) -> Option<Entity>` | two colliders start/stop touching (solid contact; triggers send the messages below) |
| `TriggerEntered` / `TriggerExited` | `trigger, other, other_body: Entity`; `actor() -> Entity` (= `other_body`) | a body (or bare collider) enters/leaves a trigger, once per body (its trigger colliders apart from its solid ones) |
| `ContactForce` | `a, b, body_a, body_b, force: f32, direction: Vec2, started: bool` | a contact pushes above `with_force_events` |
| `JointBroken` | `joint, a, b: Entity` | a joint breaks |
| `MovementEvent` | `entity`, `kind: MovementEventKind` (`Jumped`, `AirJumped`, `WallJumped`, `Dashed`, `Landed`; `#[non_exhaustive]`) | a movement preset's character jumped, dashed or landed |
| `PickedUp` | `pickup, by: Entity` | a `Pickup` was collected (`by` is the body) |
| `Killed` | `entity, zone: Entity` | something entered a `KillZone` |
| `CheckpointReached` | `checkpoint, entity: Entity` | a `Checkpoint` moved `entity`'s `Respawn` point |
| `TweenStarted` | `entity` | a `Tween` begins |
| `TweenCompleted` | `entity, finished: bool` | each iteration; `finished` on the last |
| `KnockbackStarted` / `KnockbackFinished` | `entity` | written in `FixedSet::Movement`; a finish once however it ends (decayed, `stop()`, a head-on hit) |
| `SquashStretchStarted` / `SquashStretchFinished` | `entity` | |
| `AssetEvent<T>` (one per asset type) | `Added { id }`, `Modified { id }`, `Removed { id }`, `Failed { id, error }`, `ReloadFailed { id, error }` | the asset pump (`PreUpdate`) put an asset in its store, changed it, freed it, or could not load or reload it; `load_now` sends its own at once (see [Assets](#assets)) |
| `ParticleCollision` | see [Particles](#particles) | a particle met a collider, in the particle step (`FixedSet::Particles`), so a scene's `update` reads it on the next step |
| `EmitterFinished` | `entity` | an emitter stopped (or ended its one cycle) and its last particle died, once per finish, in the particle step (see [Particles](#particles)) |
| `SkeletonEvent` | `entity`, `track: usize`, `animation: Arc<str>`, `name: Arc<str>`, `int: i32`, `float: f32`, `string: Arc<str>`, `audio: Option<SkeletonEventAudio>`, `time: f32` | a skeleton's animation crossed an event key, in the skeletons' step (`FixedSet::Skeletons`), so a scene's `update` reads it on the next step; a step's events are in the order they happened (see [Skeletons](#skeletal-animation)) |
| `SkeletonTrackEvent` | `entity`, `track: usize`, `entry: SkeletonTrackEntryId`, `animation: Arc<str>`, `kind: SkeletonTrackEventKind` (`Start`, `Interrupt`, `End`, `Complete`, `Dispose`) | something happened to a track entry, in the skeletons' step; what a call causes is sent with the skeleton's next step |
| `MapEvent` | `Spawned { map }`, `Reloaded { map }`, `Failed { map, error: Arc<AssetError> }` | a tile map landed whole (layers, objects, colliders, shadows), followed a saved file, or did not load (see [Tilemaps](#tilemaps)) |
| `FloorChanged` | `entity`, `from: Floor`, `to: Floor` | a `Climber` changed storey halfway along a tile map's stairs, in `FixedSet::Movement` (see [Tilemaps](#tilemaps)) |
| `ChunkEvent` | `Loaded { map, chunk: IVec2 }`, `Unloading { map, chunk, changed: Option<ChunkTiles> }` | a streamed map's chunk landed (its class tiles in), or unloaded: gone from the map by the time it is read, `changed` holding its tiles if the player changed it (see [Tilemaps](#tilemaps)) |
| `UiEvent` | `node: Entity, name: Option<String>, action: Option<String>, kind: UiEventKind` | a UI node was clicked, changed, dropped on, focused ... (see [Node trees](#node-trees)); also triggered on the node as an entity event |
| `UiListBind` | `list, row: Entity, index: usize` | a list node gave a pooled row an item, at the start of the UI pass; the row is drawn from the next frame; also triggered on the row |
| `SaveFinished` | `slot: String`, `op: SaveOp` (`Save`, `Delete`), `result: Result<(), Arc<SaveError>>` | a save or delete finished (`PreUpdate`), once per call, in a frame after it; saves of one slot overtaken before they were written report `Ok` |
| `SettingsSaveFailed` | `error: Arc<SaveError>` | `system.json` could not be written (`PreUpdate`); the settings still apply, and the engine tries again at the next change and at quit |

- Scenes: `ctx.messages::<T>()` returns a `Vec<T>` of what arrived since this scene last read `T` (each scene sees each message once); `ctx.write_message(m)`.
- Systems: `MessageWriter<T>::write(m)`, `MessageReader<T>::read()`.
- Buffers advance in `First`, but only once a fixed step has run since the last advance: a message stays readable until two fixed steps have run after it was written, so every reader, per-frame or fixed-step, sees it at any frame rate. While no fixed step runs (the game clock paused) nothing is dropped.
- Game types: `#[derive(Message, Clone)]`; register with `EngineBuilder::add_message::<T>()` or let the first `write_message` do it. See the `ryo-messages` skill.

---

## Input

`Input` is a resource and `ctx.input`. Keys are winit's `KeyCode` (`KeyCode::KeyA`, `Space`, `ArrowLeft`, `Escape`, `Digit1`, ...), buttons `MouseButton` (`Left`, `Right`, `Middle`).

| Area | Methods |
|---|---|
| actions | `actions() -> &Actions`, `actions_mut() -> &mut Actions` (`ctx.actions()` in a scene), `action_pressed(&str)`, `action_held(&str)`, `action_released(&str)` |
| keyboard | `key_down(KeyCode)`, `key_pressed(KeyCode)`, `key_released(KeyCode)`, `key_repeat(KeyCode)` (press or OS repeat) |
| mouse | `mouse_down(MouseButton)`, `mouse_pressed(..)`, `mouse_released(..)`, `mouse_x()`, `mouse_y()`, `mouse_position() -> (f32, f32)` (screen space: logical pixels or the pixel-art target, outside `0..screen_size` over the bars; `ctx.camera().screen_to_world(Vec2::new(x, y))` for world), `mouse_delta()`, `scroll_delta()` (lines, +y up: a wheel's lines plus a trackpad's logical pixels / 120, the same on every display), `scroll_pixels() -> (f32, f32)` (a trackpad's movement in logical pixels this frame, 0 for a wheel, +y up: follow the fingers with this; on the web a browser reports a mouse wheel as pixels too, about 100 a notch, so it is non-zero there) |
| devices | `last_device() -> InputDevice` (the newest key press, mouse press, wheel or motion, or pad press or stick past 0.5), `pad_family() -> PadFamily` |
| text and IME | `text_input() -> &str`, `has_text_input()`, `consume_text_input()`, `ime_preedit()`, `ime_preedit_cursor()`, `ime_composing()`, `ime_enabled()` |
| gamepad | `gamepad_connected()`, `gamepad_name()`, `gamepad_button_down/pressed/released(GamepadButton)`, `stick_direction_down/pressed/released(StickDirection)` (digital past 0.5, released below 0.4), `left_stick()`, `right_stick()` (deadzoned, plus `_x`/`_y`), `left_stick_raw()`, `right_stick_raw()`, `left_trigger()`, `right_trigger()` (0..1) |
| consumption | `consume_key(KeyCode)`, `consume_mouse(MouseButton)`, `consume_gamepad_button(..)`, `consume_text()`, `consume_scroll()`, `key_pressed_and_consume(..) -> bool`, `mouse_pressed_and_consume(..)`, `gamepad_button_pressed_and_consume(..)`, `consume_key_press`, `consume_mouse_press`, `consume_gamepad_button_press`, `clear_keyboard_state()` |
| gating | `block_pointer()`, `block_all()`, `unblock()` |
| engine plumbing | `new()`, `queue(InputEvent)`, `begin_frame()`, `set_fixed_step(u32)`, `end_fixed_steps(bool)`, `fixed_step()` |

- One-shots (`*_pressed`, `*_released`, action presses) are seen once per frame by per-frame code and by the first fixed step of a frame; a press in a frame that runs no step (except while the game clock is paused, when it is dropped) waits for the next step.
- A consumed input reads as not pressed (held state included) for the rest of the frame. The UI pass consumes what widgets and navigation use before `update` runs; an action binding the UI used stays hidden until it is released.

| Actions | |
|---|---|
| `Actions` (in `Input`) | `bind(name, impl IntoIterator<Item = Binding>) -> &mut Self` (the defaults; bindings the player changed, or saved, stay), `bindings(name) -> &[Binding]`, `set_bindings(name, Vec<Binding>)`, `rebind(name, slot, Binding) -> Option<String>` (appends past the end; another action of the same group with that binding swaps, its name returned), `reset_defaults()`, `names()`, `Actions::group(name) -> &str` (`"ui"` for `ui.*`, else `"game"`) |
| `enum Binding { Key(KeyCode), Mouse(MouseButton), Pad(GamepadButton), Stick(StickDirection) }` | `parse(&str) -> Option<Binding>` and `to_config() -> String` (`"Key:Space"`, `"Mouse:Left"`, `"Pad:South"`, `"Stick:LeftUp"`), `is_saveable() -> bool`, `label(PadFamily) -> String` (what a prompt shows: `"J"`, `"Enter"`, `"↑"`, `"LMB"`, `"A"`/`"×"`/`"B"` for South, `"L↑"`) |
| `StickDirection { stick: Stick, direction: Direction }` | `enum Stick { Left, Right }`, `enum Direction { Up, Down, Left, Right }` |
| `enum InputDevice { Mouse (default), Keyboard, Gamepad }` | prompts and the focus ring follow it |
| `enum PadFamily { Xbox (default), PlayStation, Nintendo }` | `from_name(&str)`: PlayStation for "dualsense", "dualshock", "playstation", "sony", "ps4", "ps5"; Nintendo for "nintendo", "switch", "pro controller", "joy-con" |

- The UI's actions are bound by the engine and may be rebound: `ui.up`, `ui.down`, `ui.left`, `ui.right` (arrows, D-pad, left stick), `ui.confirm` (Enter, numpad Enter, Space, South), `ui.back` (Escape, East), `ui.next` (Tab; Shift goes back), `ui.tab_next` (E, right bumper), `ui.tab_prev` (Q, left bumper).
- Saved bindings: with `EngineBuilder::with_user_preferences([UserPreference::Bindings])` the bindings that differ from the defaults are kept in `system.json`, loaded at startup and written with the settings a second after they change.
- `enum GamepadButton { South, East, West, North, LeftBumper, RightBumper, LeftTrigger, RightTrigger, LeftStick, RightStick, Select, Start, Guide, DPadUp, DPadDown, DPadLeft, DPadRight }`.
- `enum InputEvent { Key { key, pressed }, Text(String), MouseButton { button, pressed }, MouseMove { x, y }, RawMouseMotion { dx, dy }, Scroll(MouseScrollDelta) (a wheel's lines), ScrollPixels { x, y } (a trackpad already in logical pixels; Scroll(PixelDelta) is winit's physical pixels, divided by the window's scale factor), ImeEnabled, ImeDisabled, ImePreedit { text, cursor }, ImeCommit(String), GamepadConnected { id, name }, GamepadDisconnected(id), GamepadButton { id, button, pressed }, GamepadTrigger { id, button, value }, GamepadAxis { id, axis, value }, PadButton { button: GamepadButton, pressed }, PadStick { stick: Stick, x, y } }`: what the window, gamepads, `RYO_INPUT` scripts and the headless harness queue. `PadButton` and `PadStick` carry no gilrs id (scripted and headless input): they drive the tracked pad, or a virtual pad named "Virtual pad" when none is connected.

---

## Audio

A mixer on kira 0.12. Clips, sound definitions, music and the mixer are registry assets (see [Assets](#assets)); the engine's `AudioEngine` resource plays them, reached as `ctx.audio() -> AudioView<'_>`, which borrows the world. With no device (headless, or a device that failed to open) the same mixer runs silently on kira's mock backend. Every change to a voice or bus is smoothed by at least 10 ms, and fades to and from silence are shaped (eased, about -3.75 dB halfway) so a crossfade holds its level. A mixer that has landed or was set is built before the next play. The guide is `ryo-audio`.

| `AudioView` (`ctx.audio()`; `AudioView::new(&mut World)` in an exclusive system or a test) | |
|---|---|
| `play<'a>(&mut self, sound: impl Into<Playable<'a>>) -> Voice` | a `&Handle<Sound>` (played as the definition says) or a `&Handle<AudioClip>` (the empty definition: `sfx`, 0 dB, no limit, priority 128, one start a frame). A loop whose sound is still loading (and any play of a definition still loading) waits, reporting playing, and starts once it is ready, counting against its `per_frame` as it lands; a one-shot still loading is refused (warned once in debug builds). Failed (warned once), past its `per_frame`, cooling down or refused by a limit: the voice returned is already stopped. A loop its bus's cap, or a limit whose rule ranks (`StopQuietest`, `StopFarthest`, `StopLowestPriority`), has no room for goes virtual instead (`Voice::is_virtual`). A voice whose `stop` was called holds no room under a limit or `per_frame` |
| `play_at<'a>(&mut self, sound, position: Vec2) -> Voice` | heard from `position` by the sound's `spatial`; a one-shot at or beyond its maximum distance is culled, a loop there is virtual until the listener comes within 95% of it |
| `play_with<'a>(&mut self, sound, options: PlayOptions) -> Voice` | with a place, `persist`, extra gain or pitch |
| `play_emitter(&mut self, entity: Entity) -> Voice` | the entity's `AudioEmitter`, following it; a sound still loading waits as `play`'s does, starting where the entity is; stopped voice if it has none or is disabled |
| `play_music(&mut self, music: &Handle<Music>)` | the one music player: crossfades over the new track's `crossfade` (fades in over `fade_in` from silence); the track already playing plays on (a paused one resumes); a loading track takes over once loaded; a failed one warns once and changes nothing |
| `stop_music(&mut self, fade: Duration)`, `pause_music(&mut self)`, `resume_music(&mut self)`, `current_music(&self) -> Option<AssetId<Music>>` | `stop_music` also forgets a track still loading; `current_music` is the track playing, paused or waiting to load |
| `set_bus_volume(&mut self, bus: &str, db: f32)` | in place of the mixer's volume for the session, through reloads (at most +24); the setting, snapshot and duck still apply |
| `snapshot(&mut self, name: &str)`, `clear_snapshot(&mut self)` | blend to the mixer's snapshot over its `blend`, or back; an unknown name warns once |
| `set_mixer(&mut self, mixer: Mixer)` | build the buses from a mixer made in code, at once; one that fails its check is warned about and changes nothing |
| `seed(&mut self, seed: u64)` | the same plays pick and draw the same from the same seed (replays, tests, captures); headless runs and `testing::audio_world` start seeded |
| `waiting_for_gesture(&self) -> bool` | on the web, until the first click, tap or key (not Escape; never a pad) opens the device: one-shots are refused, loops and music wait. False natively |
| `voices_on(&mut self, bus: &str) -> usize` | voices playing on `bus` and every bus under it, each taking a voice (the virtual ones are not counted; 0 for no such bus); builds a mixer that has landed first |
| `virtual_voices_on(&mut self, bus: &str) -> usize` | the virtual loops on `bus` and every bus under it (0 for no such bus) |

| `Audio<'w, 's>` (a `SystemParam`: `mut audio: Audio` in a system) | |
|---|---|
| `play<'a>(&mut self, sound: impl Into<Playable<'a>>) -> Voice`, `play_at<'a>(&mut self, sound, position: Vec2) -> Voice`, `play_with<'a>(&mut self, sound, options: PlayOptions) -> Voice`, `play_emitter(&mut self, entity: Entity) -> Voice` | as `AudioView`'s, deferred: queued in the system's buffer and applied where its commands apply (in a scene's section: the scene's voice; from a phase's system: the engine's). The voice returned reports playing until then, takes changes that apply after its start (a plain one is heard from its first audio), and is stopped from then if the play is refused. A `play_emitter` whose entity is not an emitter yet as it applies (spawned by the same system's `Commands`, declared after `Audio`) waits for the next audio update, and is refused there if it still is not |
| `snapshot(&mut self, name: &str)`, `clear_snapshot(&mut self)`, `set_bus_volume(&mut self, bus: &str, db: f32)` | as `AudioView`'s, as the buffer applies |

`Audio` reads the engine and never borrows it mutably, so systems that play sound do not conflict. In a world without an `AudioEngine` every voice it returns is already stopped.

- `Playable<'a>` (`ryo2d::core::audio`; only the signatures name it): `Sound(&Handle<Sound>)`, `Clip(&Handle<AudioClip>)`, `From` both, so pass `&handle`.
- `PlayOptions { position: Option<Vec2>, persist: bool, volume_db: f32, pitch: f32 }` (`Copy`, `Default`: changes nothing): `persist` makes the voice the engine's, so the scene's exit leaves it playing; `volume_db` and `pitch` (semitones) add to the definition's.
- `Voice` (`Clone`, `Default` = never started, `Debug`): `stop(&self, fade: Duration)` (at least 10 ms), `pause(&self)`, `resume(&self)`, `set_volume(&self, db: f32)` and `set_pitch(&self, semitones: f32)` (relative to the sound's level and pitch, as `PlayOptions` are: a game layer over the definition, its drawn spread and the play's options, which a definition's reload still moves; the sums kept within -60 to +24 dB and 48 semitones; pitch changes speed too), each over 10 ms; `pause_over`, `resume_over(&self, fade: Duration)`, `set_volume_over(&self, db: f32, fade: Duration)`, `set_pitch_over(&self, semitones: f32, fade: Duration)` over a fade of their own (at least 10 ms). `is_playing(&self) -> bool` (a voice held by the game's pause or its scene's hold still counts, as does a virtual one), `is_stopped(&self) -> bool` (ended or never started), `is_virtual(&self) -> bool` (a loop keeping its place silently, with no voice: its bus's cap or a ranking limit had no room for it, or it is at or beyond its `max_distance`; it takes every change meanwhile, moves on through its clip, and comes back fading in over 50 ms, or its own `fade_in` if it was never heard, when it is in range and there is room, or in place of a voice it outranks), `pick(&self) -> Option<VoicePick>`. Changes apply as the scene code that made them ends, and in the frame's `audio_system` (`PostUpdate`); a plain one made in the frame the voice started applies at once, and an `_over` one ramps from the level its play started at. Dropping it never stops it. It belongs to the scene that played it (as the scene exits its loops, its paused or held one-shots and its one-shots with more than 2 s left but on `ui` stop with their `fade_out`, and its other playing one-shots finish; while a scene above does not update it, its voices on buses that pause with the game are held), to the engine (`persist`, or played outside every scene), or to its emitter's entity (held while the scene that added the emitter is held; an emitter of no scene, or whose scene has gone, while any scene is).
- `VoicePick { clip_index: usize, clip: AssetId<AudioClip>, volume_db: f32, pitch: f32 }`: what the play picked and drew.
- `Sound` (`Clone`, `Default` = the empty definition; an asset from `.sound.ron`): `clips: Vec<SoundClip>`, `bus: String` (`"sfx"`), `volume_db`, `volume_range_db`, `pitch`, `pitch_range` (semitones), `no_repeat: u8`, `limit: Option<VoiceLimit>`, `priority: u8` (0 to 255, higher more important; `Sound::DEFAULT_PRIORITY` = 128), `cooldown: Duration`, `per_frame: u8` (the most of its voices that start in one frame, wherever they play; `Sound::DEFAULT_PER_FRAME` = 1, at least 1, `per_frame: 0` in a file fails its load), `looping: bool`, `fade_in: Duration`, `fade_out: Duration` (at least 10 ms is used), `spatial: Option<Spatial>`.
- `SoundClip { clip: Handle<AudioClip>, weight: f32, region: Option<Range<f64>>, loop_region: Option<Range<f64>> }`: `new(clip)` (all of it, weight 1). `region` is seconds of the clip; `loop_region` seconds from the region's start.
- `VoiceLimit { max: u16, rule: LimitRule }`; `enum LimitRule { PreventNew, StopOldest (default), StopQuietest, StopFarthest, StopLowestPriority }`. A sound's `per_frame` refuses its extra starts in a frame before any limit. No rule stops a more important voice, or an equally important one started in the same frame. Past 96 voices on a bus's track the least important, then quietest, voice is stolen over 10 ms. `PreventNew` and `StopOldest` end loops as they end one-shots (refused, or stopped for good), their own sound's limit asked before a loop's distance and kira's room, and count their own sound's virtual loops (`PreventNew` refuses while one is virtual, `StopOldest` ends the oldest, virtual or heard; loops under them are outside the loudness ranking both ways); a loop `StopQuietest`, `StopFarthest`, `StopLowestPriority` or the cap stops or refuses goes virtual instead (a loop a bus limit picks for another sound's play goes by its own sound's rule), and comes back when there is room, or in place of a voice less important or 3 dB quieter (not under `PreventNew` or `StopOldest`, nor through a limit in place of a loop under them, never in place of music); a voice that moved stays put 250 ms. At most 1024 are virtual (past them a loop that would go virtual is refused, `virtual` in the log, and one stolen stops); one-shots and music are never virtual.
- `Spatial { min_distance: f32, max_distance: f32, curve: Falloff, pan_width: f32, low_pass_at_max: Option<f64> }`; `enum Falloff { Linear, Inverse (default), Log }` (-6, about -25 and -30 dB halfway). `falloff_gain_db(&Spatial, distance: f32) -> f32`, `pan_for(&Spatial, dx: f32) -> f32` (-1 left to 1 right). At most 32 voices carry a distance low-pass. A loop at or beyond `max_distance` is virtual (no voice) until the listener comes within 95% of it.
- `AudioListener` (component, requires `Transform`): the entity that hears; the main camera without one. `AudioEmitter { sound: Handle<Sound>, play_on_spawn: bool }` (component, requires `Transform`): plays as added and as its entity is enabled (with `play_on_spawn`), its voices following the entity and stopping (loops, paused or held one-shots) as it is despawned, disabled or loses the emitter, while its playing one-shots play out. Emitters of one sound spawned together start as its `per_frame` allows, the rest the frames after.
- `Music` (`Clone`; an asset from `.music.ron`): `volume_db`, `fade_in: Duration` (0), `crossfade: Duration` (`Music::DEFAULT_CROSSFADE`, 1 s), `tempo: Option<f32>`, `meter: Option<(u8, u8)>` (recorded, unused by the player), `bus: String` (`"music"` or under it). `Music::looping(clip: Handle<AudioClip>, loop_start: f64, loop_end: Option<f64>) -> Music`, `Music::intro_and_loop(intro: &AudioClip, looped: &AudioClip) -> Result<Music, MusicJoinError>` (joined into one sound, gapless; both decoded, one sample rate). `MusicJoinError` (`#[non_exhaustive]`): `StreamedIntro`, `StreamedLoop`, `Rates { intro, looped }`, `EmptyLoop`.
- `Mixer { buses: Vec<BusDef>, snapshots: Vec<SnapshotDef>, ducks: Vec<DuckDef>, on_focus_loss: FocusLoss }` (`Default` = the standard layout; an asset from `.mixer.ron`). The standard buses always exist: `master`, and under it `music`, `sfx`, `ui`, `voice`, `ambience` (`sfx`, `voice`, `ambience` pause with the game; `sfx`, `ambience` follow time scale). `MIXER_PATH` = `"audio/mixer.mixer.ron"`, loaded at start if present. A save or `set_mixer` builds a new tree: loops move to it, one-shots finish on the old one, which rings out its reverbs' and delays' tails (at most 10 s; at most four old trees at once) before it goes.
- `BusDef { name, volume_db, effects: Vec<EffectDef>, sends: Vec<(String, f32)>, pauses_with_game: Option<bool>, follows_time_scale: Option<bool>, limit: Option<u16> }`: `new(name)`. `enum EffectDef { LowPass { cutoff, resonance }, HighPass { cutoff, resonance }, Eq { kind: EqKind, frequency, gain_db, q }, Reverb { feedback, damping, stereo_width, mix }, Delay { seconds, feedback_db, mix }, Compressor { threshold_db, ratio, attack, release, makeup_db, mix }, Distortion { kind: DistortionKind, drive_db, mix } }`; `enum EqKind { Bell, LowShelf, HighShelf }`, `enum DistortionKind { HardClip (default), SoftClip }`; `enum FocusLoss { Keep (default), Duck(f32), Mute }`.
- `SnapshotDef`, `SnapshotBus`, `DuckDef` (a mixer made in code): `SnapshotDef { name, blend: f32, buses: Vec<(String, SnapshotBus)> }`, `SnapshotBus { volume_db: f32, low_pass: Option<f32> }`, `DuckDef { trigger, target, by_db: f32, attack: f32, release: f32 }`.
- `enum VolumeKind { Master, Music, Sfx, Ui, Voice, Ambience }`: `ALL`, `bus() -> &'static str`, `default_value() -> f32` (1 for every kind, the mix as designed). `volume_to_db(v: f32) -> f32`: 40·log10(v) (amplitude v²): 1 is 0 dB, 0.5 about -12 dB, 0 silent (-60). The player's volume settings are `Settings`: `ctx.settings().set_volume(kind, v)` and `volume(kind)` (see [Config, settings and saves](#config-settings-and-saves)); playing sounds follow in the frame they change.
- `AudioClip` (`Clone`; `.wav`, `.ogg`, `.mp3`, `.flac`, mono or stereo): `duration() -> Duration`, `sample_rate() -> u32`, `is_streamed() -> bool`, `from_frames(sample_rate: u32, frames: &[[f32; 2]]) -> Result<AudioClip, AudioClipError>` (`AudioClipError::ZeroSampleRate` for a rate of 0, `#[non_exhaustive]`). `AudioClipSettings { mode: ClipMode }`, `enum ClipMode { Auto (default: decode up to 10 s, stream longer), Decode, Stream }` (the web decodes every clip). Loaders: `AudioClipLoader`, `SoundLoader`, `MusicLoader`, `MixerLoader`.

See `ryo-audio`.

---

## Cameras

Cameras are entities: a `Camera2d` with a `Transform` (its position is the view's centre in world space, its rotation turns the view). The engine spawns the main camera (`MainCamera`, `Interpolated`) at the centre of the design area with `Projection::Fit` over it; it survives world cleanup, and `SceneCleanup::CAMERA` resets its position, zoom, rotation, projection and shake. Spawn more cameras for split screen, minimaps or render-to-texture.

```rust,ignore
// Fields, and the types they use
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
- `RenderLayers` (component on drawables and cameras): `layer(n)` (0..=31), `all()`, `none()`, `with(n)`, `intersects(&other)`. Default `layer(0)`. A camera draws the entities, composite tiles, particle emitters, trails and lines that share a layer with it; `Scene::render` sprites, parallax and debug overlays are on layer 0.

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

## Images

`Image` (an asset: RGBA pixels, 4 bytes a pixel, rows from the top; clones share their pixels until one changes its own). Sprites, particles, parallax, trails and the UI draw a `Handle<Image>`.

| | |
|---|---|
| `ctx.assets().load_now::<Image>(path)`, `load::<Image>(path)`, `load_now_with::<Image, _>(path, ImageSettings { svg: SvgSize::Fit { width, height } })` | from a file in `assets/`: PNG, JPEG, BMP, GIF, WebP, TGA, TIFF, QOI, ICO, or SVG at a size (see [Assets](#assets)) |
| `from_rgba(Vec<u8>, w, h)`, `solid_rect(w, h, r, g, b, a)`, `solid_circle(diameter, r, g, b, a)`, `solid_ellipse(w, h, r, g, b, a)`, `decode(&[u8]) -> Result<Image, ImageError>` (raster bytes, such as `include_bytes!`) | made in code; `ctx.assets().add(image)` gives its handle |
| `width()`, `height()`, `data() -> &[u8]`, `data_mut() -> &mut [u8]`, `is_opaque()`, `scaled(f32)`, `scaled_xy(sx, sy)`, `draw(..)`, `draw_scaled(..)`, `draw_scaled_xy(..)` | |

An image that is not loaded (not yet, failed or freed) draws a magenta-and-black checker in debug builds and nothing in release ones. `ImageError { Decode(image::ImageError) }`.

`AnimatedSprite` (CPU sheet player): `from_sheet(Image, frame_count, fps)`, `update(dt)`, `draw(..)`, `frame_size()`.

`Gpu { pub device: Arc<wgpu::Device>, pub queue: Arc<wgpu::Queue> }` is a resource once the GPU is up.

---

## Sprite animation

| Type | API |
|---|---|
| `Animation` | `new(name, frames: Vec<Handle<Image>>, frame_duration: f32)`, `with_frame_durations(name, frames, durations: Vec<f32>)`, `with_loop_mode(LoopMode)`, `name()`, `duration()`, `frame_count()`, `frame(i)`, `frame_duration(i)`, `loop_mode()` |
| `Animator` (component, engine-driven) | `new(Animation)`, `set_animation(Animation)`, `play`, `pause`, `stop`, `reset`, `set_speed(f32)`, `speed()`, `is_playing()`, `just_finished()`, `current_sprite()`, `current_frame_index()`, `animation_name()`, `update(dt)` |
| `AnimationController` (component, engine-driven) | `new()`, `add_animation(AnimationId, Animation)`, `set_state(AnimationId)`, `transition_after_finish(AnimationId)`, `current_state()`, `just_finished()`, `current_sprite()`, `reset`, `pause`, `play`, `set_speed`, `update(dt)` |
| `ValidatedAnimator` (engine-driven on an entity, like `Animator`) | `new(Animation, AnimationId)`, `with_animation(Animation, AnimationId)`, `with_transition(from, to)`, `with_bidirectional(a, b)`, `with_transition_from_any(to)`, `set_animation(id) -> bool`, `force_animation(id)`, `transition_after_finish(id) -> bool`, `current()`, `current_sprite()`, `just_entered()`, `just_finished()`, `can_transition_to(id)`, `time_in_state()`, `pause`, `play`, `set_speed`, `reset`, `update(dt)` |

`type AnimationId = &'static str`; `enum LoopMode { Once, Loop, PingPong, Times(u32) }`; `enum PlaybackState { Playing, Paused, Stopped }`. The engine advances `Animator`, `AnimationController` and `ValidatedAnimator` each fixed step and puts the current frame on the entity's `Renderable`. See `ryo-animation`.

---

## GPU sprites and parallax

`Scene::render(&mut self, gpu: &mut GpuRenderContext)` adds batched sprites for the frame; they sort with entities by `z_order`.

```rust,ignore
// Fields
pub struct GpuRenderContext<'a> { pub batch: &'a mut BatchCollector, pub world: &'a World, /* .. */ }
```

| `GpuRenderContext` | |
|---|---|
| `add_sprite(&Handle<Image>, instance: SpriteInstance, draw: SpriteDraw)` | one sprite, in world space (the instance's centre is a world position; for something fixed on screen, place it with `camera().screen_to_world`); its texture uploads when first drawn and is freed with the image's last handle |
| `add_custom_sprite(&Handle<Image>, &Handle<Shader>, &ShaderParams, SpriteInstance, SpriteDraw)` | one sprite with a game shader |
| `screen_size()`, `screen_width()`, `screen_height()`, `camera() -> &CameraView` (the main camera's view) | |

- `SpriteInstance { transform, uv_rect, tint, effect_params, effect_extra, uv_bounds }`: `new()`, `with_transform(x, y, rotation, width, height)` (the quad's centre and size in world units), `with_uv(u_min, v_min, u_max, v_max)`, `with_tint(Color, alpha)`, `with_effect(&SpriteEffect)`, `with_emission(f32)`, `emission() -> f32` (default 1; kept in the transform's last lane as `emission - 1`).
- `SpriteDraw { z: i32, blend: BlendMode, filter: Filter, lit: bool, effects: bool, emission: f32 }` (`Default`: z 0, `Alpha`, `Filter::Default`, lit, no effects, emission 1; `effects` when the instance carries `with_effect`; `with_emission(f32)` multiplies the instance's emission): `SpriteDraw { z: 5, blend: BlendMode::Additive, ..Default::default() }`.
- `BatchCollector`: `new()`, `with_capacity(n)`, `add(&Handle<Image>, SpriteInstance, SpriteDraw)`, `sprite_count()`, `clear()`.

Parallax layers (components; spawn one entity per layer):

- `ParallaxLayer { sprite: Handle<Image>, scroll: Vec2, auto_scroll: Vec2, offset: Vec2, tile: TileMode, scale, tint, z_order, blend, filter, lighting, scrolled: Vec2 }`: `new(&Handle<Image>)` (fixed on screen at its top-left, untiled, scale 1, white, lit), `with_scroll(Vec2)` (per axis: 0 stays on screen, 1 moves with the world), `with_auto_scroll(Vec2)` (world units per second, game time), `with_offset(Vec2)` (positive moves it right and down), `with_tile(TileMode)`, `with_scale(f32)`, `with_tint(Color)`, `with_z_order(i32)`, `with_blend(BlendMode)`, `with_filter(Filter)`, `with_lighting(bool)`. `scrolled` is how far it has auto-scrolled (set it to carry a backdrop across scenes).
- Every camera on its `RenderLayers` lays it out from its own view, so split screen and minimaps each get their own parallax.
- `enum TileMode { None, Horizontal, InfiniteScroll }`.

---

## Composite tiles

Multi-part sprites laid out from the entity's position (parts at offsets from it, not anchored).

- `CompositeTileBuilder`: `new()`, `cap(sprite, offset_x, offset_y)`, `cap_flipped_y(..)`, `cap_flipped(sprite, x, y, flip_x, flip_y)`, `static_part(..)`, `body_tiled(sprite, x, y, TileDirection, target_size)`, `body_tiled_flipped(..)`, `tiled_part(..)`, `z_order(i32)`, `visible(bool)`, `build() -> CompositeTile`, `build_with_collider(&Assets<Image>) -> (CompositeTile, Collider)` (a rect collider offset over all the parts, sized from the images, so build it once they are loaded).
- `CompositeTile { parts: Vec<TilePart>, visible, z_order }` (component): `new()`, `add_part(TilePart)`, `with_visible`, `with_z_order`, `bounds(&Assets<Image>) -> (f32, f32, f32, f32)`, `size(&Assets<Image>)`.
- `TilePart { sprite: Handle<Image>, offset_x, offset_y, tiling: Option<(TileDirection, f32)>, flip_x, flip_y }`: `static_part(sprite, x, y)`, `tiled_part(sprite, x, y, direction, target_size)`, `with_flip_x`, `with_flip_y`, `is_tiled()`, `bounds(&Assets<Image>)`.
- `enum TileDirection { Up, Down, Left, Right }`: `offset(tile_size)`, `is_horizontal()`, `is_vertical()`.

---

## Physics

Rapier2D 0.36. A `Collider` joins the simulation on its own (a fixed collider) or attaches to the nearest `RigidBody` at or above its entity. `PhysicsConfig::pixels_per_meter` px = 1 m (default 100; choosing it: ryo-physics); gravity is in px/s². Positions are pixels, +Y down. Everything below is in the prelude.

| Type | API |
|---|---|
| `RigidBody` (component, brings `Transform`, `Velocity`, `AngularVelocity`) `{ kind: BodyKind, gravity_scale, linear_damping, angular_damping, lock_rotation, ccd, can_sleep, dominance: i8 }` | `dynamic()` (also `default()`), `fixed()`, `kinematic()`, `with_gravity_scale(f32)`, `with_linear_damping(f32)`, `with_angular_damping(f32)`, `lock_rotation()`, `with_ccd()`, `never_sleep()`, `with_dominance(i8)` |
| `enum BodyKind { Dynamic, Fixed, Kinematic }` | |
| `AngularVelocity(pub f32)` (component, rad/s, same sign as `Transform.rotation`) | written back from awake dynamic bodies; set it to spin one |
| `Collider` (component) `{ shape: ColliderShape, offset: Vec2, rotation: f32, is_trigger, layer: u32 (default 1), mask: u32 (default all), density: f32 (1.0), mass: Option<f32>, friction (0.5), restitution (0.0), force_threshold: Option<f32>, one_way: Option<Vec2> (None: solid everywhere) }` | `rect(w, h)`, `circle(r)`, `capsule(height, radius)`, `polygon(impl IntoIterator<Item = Vec2>)`, `segment(a, b)`, `polyline(points)`, `segments(points, impl IntoIterator<Item = [u32; 2]>)` (edges by point index, joined or not), `with_offset(x, y)`, `with_rotation(radians)`, `as_trigger()`, `with_layer(u32)`, `with_mask(u32)`, `with_density(f32)`, `with_mass(f32)`, `with_friction(f32)`, `with_restitution(f32)`, `with_force_events(threshold)`, `one_way()` (solid only from above), `one_way_towards(direction)` (a local direction; zero is solid and warned), `world_bounds(&GlobalTransform) -> (Vec2, Vec2)`. Its storey is its entity's `Floor`, else its body's, else the ground floor |
| `enum Floor { On(u8), Every }` (component, `Copy`, `Eq`, `Hash`; `default()` is `On(0)`) | a storey (not the ground underfoot that `CharacterState::floor` names): `GROUND` (`On(0)`, where everything without a `Floor` is), `meets(Floor) -> bool` (a number meets itself, `Every` meets all), `number() -> Option<u8>` (`None` for `Every`). Colliders on floors that do not meet never interact (contacts, trigger messages, the character sweep), independent of the 32 layer and mask bits; change it at any time and the next step filters again. Its `Transform` stays where it stands on the ground: on a map with floors it draws lifted by its storey's height, with everything below it in the hierarchy (`MapsView::drawn_lift`; lines and the F2 overlay stay in ground space). A `Light2D` on a storey is blocked only by the `LightOccluder2D`s on a storey that meets it (an occluder without one is on the ground floor), and draws lifted with its shadows |
| `enum ColliderShape` | `Rect { size: Vec2 }`, `Circle { radius }`, `Capsule { height, radius }` (upright, `height` includes the ends), `Polygon { points }` (convex hull), `Segment { a, b }`, `Polyline { points }` (open chain), `Segments { points, segments: Vec<[u32; 2]> }` (edges between points by index: several outlines in one collider, as a tile map packs a chunk's) |
| `Joint` (component) `{ a: Entity, b: Entity, kind: JointKind, anchor: JointAnchor, limits: Option<(f32, f32)>, motor: Option<JointMotor>, collisions: bool, break_force: Option<f32> }` | `revolute(a, b)`, `prismatic(a, b, axis: Vec2)`, `fixed(a, b)`, `rope(a, b, max_length)`, `spring(a, b, rest_length, stiffness, damping)`, `at(Vec2)`, `with_anchors(a: Vec2, b: Vec2)`, `with_limits(min, max)`, `with_motor_velocity(target, max_force)`, `with_motor_position(target, stiffness, damping)`, `with_collisions()`, `with_break_force(f32)` |
| `enum JointKind` | `Revolute`, `Prismatic { axis }`, `Fixed`, `Rope { max_length }`, `Spring { rest_length, stiffness, damping }` |
| `enum JointAnchor` | `Origins` (default), `World(Vec2)`, `Local { a: Vec2, b: Vec2 }` |
| `enum JointMotor` | `Velocity { target, max_force }`, `Position { target, stiffness, damping }` |
| `PhysicsConfig` (resource) `{ gravity: Vec2, pixels_per_meter: f32, solver_iterations: usize }` | `default()` ((0, 980), 100, 4), `with_gravity(Vec2)`, `zero_gravity()`, `with_pixels_per_meter(f32)`, `with_solver_iterations(usize)` |
| `TriggerContents` (component, engine-added to every trigger) | `entities() -> &[Entity]` (bodies inside, in entry order), `contains(e)`, `len()`, `is_empty()` |
| `QueryFilter` | `new()` / `default()` (every layer on every storey, skips triggers), `with_mask(u32)`, `excluding(Entity)` (repeatable), `with_triggers()`, `on_floor(Floor)` (only colliders on a floor that meets it: that storey's and those on `Floor::Every`) |
| `Hit { collider, body, point: Vec2, normal: Vec2, distance: f32 }` | a ray or shape cast's first contact |
| `Overlap { collider, body }` | an overlap test result |
| `PointHit { collider, body, point: Vec2, inside: bool }` | `nearest` result |
| `Touch { collider, body, point, normal, impulse }` | a solid contact on an entity: the other side, the deepest point, the normal from the entity toward the other side (ground below is `(0, +1)`), the step's normal impulse |
| `Contact { point: Vec2, normal: Vec2 }` | where two colliders first touched; the normal points from `a` to `b` |

`Physics` (resource; `ctx.physics() -> &Physics`, `ctx.physics_mut() -> Mut<Physics>`). Queries see the world as the last step left it.

| Group | Methods |
|---|---|
| Rays and casts | `raycast(origin, direction, max_distance, &QueryFilter) -> Option<Hit>`, `raycast_all(..) -> Vec<Hit>` (nearest first), `shape_cast(&ColliderShape, origin, rotation, direction, max_distance, &QueryFilter) -> Option<Hit>` |
| Overlaps | `overlap_point(point, &QueryFilter) -> Vec<Overlap>`, `overlap_shape(&ColliderShape, position, rotation, &QueryFilter) -> Vec<Overlap>`, `nearest(point, max_distance, &QueryFilter) -> Option<PointHit>` |
| Contact state | `contacts(entity) -> Vec<Touch>`, `overlaps(entity) -> Vec<Overlap>`, `in_contact(a, b) -> bool`, `is_sleeping(entity) -> bool`, `mass(body) -> Option<f32>` |
| Forces | `apply_impulse(body, Vec2)`, `apply_impulse_at(body, impulse, point)`, `apply_angular_impulse(body, f32)`, `apply_force(body, Vec2)` (next step only), `apply_force_at(body, force, point)`, `apply_torque(body, f32)` (next step only) |

An entity argument is a body (all its colliders) or one collider.

Messages (read with `ctx.messages::<T>()`), all with `a, b` (colliders), `body_a, body_b` and the helpers `involves(e)`, `other(e) -> Option<Entity>`, `other_collider(e) -> Option<Entity>` except `JointBroken`:

| Message | Fields |
|---|---|
| `CollisionStarted` | `contact: Option<Contact>` |
| `CollisionStopped` | either entity may already be despawned |
| `ContactForce` | `force: f32`, `direction: Vec2`, `started: bool`; sent every step above the collider's `with_force_events` threshold |
| `JointBroken` | `joint`, `a`, `b` (bodies); the `Joint` component is removed |

- Collider geometry is centred on the entity's origin plus `offset`, which is the sprite's centre with the default `Anchor::Center`.
- Move a dynamic body through `Velocity` / `AngularVelocity`, forces or impulses; a game write to `Transform` teleports it. A kinematic body travels to a written `Transform` over the step (it wins over `Velocity` when both are written; an unchanged pose does not count), except a character (`CharacterController`): a written `Transform` teleports it, its `CharacterState` resets, and it moves on by its `Velocity` from the new pose that step. Physics writes pose and velocities back after each step.
- Physics pauses while the top scene's `is_physics_paused()` is true; queued forces and impulses wait and apply together at the next step. `time.game.max_delta()` / `set_max_delta` (250 ms) caps the game time a frame adds, after speed scaling. See `ryo-physics`.
- Collision and trigger messages are written in the physics step, after `Scene::update`: `update` reads them on the next step. Triggers report through `TriggerEntered` / `TriggerExited` (see [Messages](#messages)), not the collision messages.

---

## Characters and gameplay

The character controller and what is built on it: movement input, the platformer and top-down presets, trigger behaviours and tile colliders. All in the prelude; usage in `ryo-character`. Components marked non-exhaustive are built with their constructors and builders, never struct literals.

| Type | API |
|---|---|
| `CharacterController` (component, non-exhaustive; brings `Transform`, `Velocity`, `CharacterState` and a kinematic `RigidBody` with locked rotation) `{ mode: CharacterMode, max_slope: f32 (radians), step_height: Option<f32>, snap_to_ground: Option<f32>, skin: Option<f32>, push: bool, drop_through: bool }` | `platformer()` (also `default()`), `top_down()`, `with_max_slope(degrees)` (default 45°), `with_step_height(px)` (default off), `with_snap_to_ground(px)` (default a fifth of the collider's height; 0 off), `with_skin(px)` (default 1% of the height), `with_push(bool)` (default true). Set `drop_through = true` to drop through the one-way platform underfoot; the engine clears it |
| `enum CharacterMode { Platformer (default), TopDown }` | platformer: floors, walls, ceilings, snapping, steps; top-down: every surface a wall |
| `CharacterState` (component, engine-written, non-exhaustive) `{ on_floor, on_wall, on_ceiling: bool, floor_normal, wall_normal: Vec2, floor: Option<Entity>, floor_one_way: bool, platform_velocity: Vec2, landed, left_floor, sliding_down_slope: bool, velocity_before: Vec2, hits: Vec<CharacterHit> }` | the last move: normals point out of the surface toward the character (flat ground `(0, -1)`); `floor` is a body or bare collider; `landed` / `left_floor` hold for one step; `velocity_before` is what the step asked for |
| `CharacterHit { entity, body: Entity, point, normal: Vec2 }` | one collider the move touched |
| `MoveIntent` (component) `{ direction: Vec2, jump_held, jump_pressed, dash_pressed: bool }` | `clamped_direction() -> Vec2` (length at most 1), `clamped_axes() -> Vec2` (each axis -1..1); both read non-finite as zero. The presets clear the pressed flags they act on |
| `MoveBindings` (component, non-exhaustive, brings `MoveIntent`) `{ left, right, up, down, jump, dash: Vec<KeyCode>, gamepad: bool }` | `default()` (A/←, D/→, W/↑, S/↓, Space, both Shifts, and the first gamepad: left stick and d-pad move, South jumps, West dashes), `with_left(impl IntoIterator<Item = KeyCode>)`, `with_right`, `with_up`, `with_down`, `with_jump`, `with_dash`, `keyboard_only()` |
| `PlatformerMovement` (component, non-exhaustive; brings `MoveIntent`, `PlatformerState`, `CharacterController::platformer()`) `{ run_speed, accel_time, decel_time, air_control, jump_height, time_to_apex, fall_multiplier, max_fall_speed, coyote_time, jump_buffer, jump_cut, knockback_control: f32, air_jumps: u32, wall_slide: Option<f32>, wall_jump: Option<WallJump>, dash: Option<Dash> }` | `default()` (180 px/s, 0.08 s, 0.06 s, 0.65, 64 px, 0.38 s, 1.5, 600 px/s, 0.1 s, 0.1 s, 0.5, 0, 0, off, off, off), `with_run(speed, accel_time, decel_time)`, `with_air_control(fraction)`, `with_jump(height, time_to_apex)`, `with_fall(multiplier, max_speed)`, `with_coyote_time(s)`, `with_jump_buffer(s)`, `with_jump_cut(factor)`, `with_knockback_control(fraction)`, `with_air_jumps(n)`, `with_wall_slide(max_speed)`, `with_wall_jump(launch: Vec2, lock_time)`, `with_dash(speed, duration, cooldown)` |
| `WallJump { launch: Vec2, lock_time: f32 }`, `Dash { speed, duration, cooldown: f32 }` | the extras' settings (`Dash` is shared with the top-down preset) |
| `PlatformerState` (component, engine-written, non-exhaustive) `{ facing: f32, jumping: bool, air_jumps_left: u32, wall_sliding, dashing, dash_ready: bool }` | `reset()` (forgets jumps, timers and dashes; keeps `facing`) |
| `TopDownMovement` (component, non-exhaustive; brings `MoveIntent`, `TopDownState`, `CharacterController::top_down()`) `{ max_speed, accel_time, decel_time: f32, dash: Option<Dash>, knockback_control: f32 }` | `default()` (160 px/s, 0.08 s, 0.1 s, off, 0), `with_speed(max_speed, accel_time, decel_time)`, `with_dash(speed, duration, cooldown)`, `with_knockback_control(fraction)` |
| `TopDownState` (component, engine-written, non-exhaustive) `{ facing: Vec2, dashing: bool }` | `reset()` |
| `Pickup` (component, non-exhaustive) `{ respawn_after: Option<f32> }` | `new()`, `respawn_after(seconds)`; on a trigger collider |
| `KillZone` (component, non-exhaustive) `{ despawn: bool }` | `new()`, `despawning()`; on a trigger collider |
| `Checkpoint` (component, non-exhaustive) `{ offset: Vec2 }` | `new()`, `with_offset(Vec2)`; on a trigger collider; moves the point to its trigger's storey (its `Floor`, else its body's; one on `Floor::Every` keeps the storey it was reached on) |
| `Respawn` (component, non-exhaustive) `{ point: Vec2 }` | `here()` (its `Transform` when the engine first sees it), `at(point)`; root entities only. A respawn also puts the entity back on the storey of its last checkpoint, or before any, the storey (`Floor`) it was on when the engine first saw its `Respawn` (it gains a `Floor` only when that is not the ground floor) |
| `TileColliders` (component, non-exhaustive, brings `Transform`) `{ tile_size: Vec2, layer, mask: u32, floor: Floor, friction, restitution: f32, solid_rects: bool }` | `new(width, height, tile_size: Vec2)` and `from_cells(width, height, tile_size, Vec<TileCell>)` (row by row from the top) hold the cells from (0, 0) up to that size; `unbounded(tile_size)` holds any signed cell; `bounds() -> Option<CellRect>` (`None` when unbounded), `extent() -> CellRect` (the bounds, or an unbounded grid's chunks' cells), `get(IVec2) -> TileCell` (`Empty` outside), `set(IVec2, TileCell) -> bool` (true when the cell changed; a cell past 186,411 cells from (0, 0) on either axis is refused, logged once), `with_layer(u32)`, `with_mask(u32)`, `with_floor(Floor)` (off the ground floor each part carries it as its `Floor`; neither the map's `Floor` nor its body's reaches the parts; changing it rebuilds them), `with_friction(f32)`, `with_restitution(f32)`, `solid_rects()` (defaults: layer 1, mask all bits, the ground floor, friction 0.5, restitution 0, outlines), `add_shape(CellShape) -> u16` (an authored shape for `TileCell::Shape` cells to name; at most 65,535, `u16::MAX` names none), `shape(u16) -> Option<&CellShape>`, `with_lattice(Affine2)` (cells placed through it, lattice units to the grid's space, instead of `tile_size` rectangles: a tile map's isometric, staggered and oblique grids give theirs, `Lattice::to_pixel`; the outlines trace on its squares and their points are mapped through it), `lattice() -> Option<Affine2>`. Its `Transform` places cell (0, 0)'s top-left (with a lattice, its lattice point (0, 0)). The colliders are child entities packed by area: the outlines starting in each 32 x 32-cell chunk are one collider, and so are a chunk's one-way runs along one row. Cells are stored by 32-cell chunk, so an empty area costs nothing, and each collider sits at its chunk's top-left with its points relative to it. A real change retraces only the outlines through the cells it changed and reshapes only their chunks' colliders, in place (same entities, contacts kept), before that step's physics. A tile map keeps these for its colliding layers ([Tilemaps](#tilemaps)) |
| `enum TileCell { Empty (default), Solid, OneWay, Slope(SlopeCorner), Shape(u16) }` | `Shape` names a shape `add_shape` returned |
| `CellRect { min: IVec2, max: IVec2 }` (`Copy`, `Default`, `Eq`) | a rectangle of signed cells, `min` included, `max` not: `new(min, max)`, `from_size(UVec2)` ((0, 0) to the size), `size() -> UVec2` (zero when empty), `is_empty()`, `contains(IVec2)`, `union(CellRect)` (an empty side is ignored). A `TileGrid`'s or a `TileColliders`' bounds and extent |
| `CellShape { polygons: Vec<Vec<Vec2>>, one_way: bool }` | polygons in tile units ((0, 0) the cell's top-left, (1, 1) its bottom-right); a solid shape joins the outlines as solid cells do (seamless with its neighbours), a one-way shape's top edges join the one-way runs; clipped to its cell, and a polygon that crosses itself collides as its hull (logged) |
| `enum SlopeCorner { BottomLeft, BottomRight, TopLeft, TopRight }` | the filled corner of a 45° half-cell; `from_name(&str) -> Option<Self>` (`"bottom-left"` .. `"top-right"`, any case) |
| `read_move_bindings` | the system that fills every bound `MoveIntent` (engine-registered) |

`FixedSet::Movement` runs, chained: the zone behaviours (`Respawn::here` points, `Pickup`, pickup respawn timers, `KillZone`, `Checkpoint`, acting on the previous step's `TriggerEntered` from solid colliders), `read_move_bindings`, knockback, the platformer and top-down presets; `TileColliders` rebuilds run in the set too, after tile maps have put their tiles' changes in their grids. Under `PhysicsPaused` the bindings, knockback, presets and pickup timers wait; the zone behaviours keep running. A game write to a character's `Transform` is a teleport: its `CharacterState` resets and it moves on by its `Velocity` from there that step.


---

## Lighting

Lighting turns on once any `Light2D` exists (or when forced) and lights the world, never the UI.

| Type | API |
|---|---|
| `LightingSettings` (resource; `ctx.lighting()`) | `set_enabled(bool)` (force on, e.g. a dark scene before any light, or off), `enabled() -> Option<bool>`, `ambient() -> &AmbientLight`, `set_ambient(AmbientLight)`, `atmosphere() -> &Atmosphere`, `set_atmosphere(Atmosphere)` |
| `Light2D` (component) | `point(Color, radius)`, `point_with_intensity(..)`, `point_full(..)`, `spot(Color, radius, angle_deg, direction_deg)`, `spot_with_intensity(..)`, `spot_full(..)`; presets `torch(r)`, `moonlight(r)`, `lamp(r)`, `flashlight(r, dir_deg)`, `warning(r)`, `magic(r, Color)`; builders `with_shadows()`, `with_soft_shadows(size_px: f32)` (casts shadows; `size_px` the radius of the light's source in pixels, 0 a hard edge, edges widening with distance from their occluder), `with_shadow_color(Color)` (its alpha the shadow's strength, applied once however many occluders cover a point), `with_intensity`, `with_falloff`, `with_volumetric()`, `with_god_rays()`, `with_volumetric_and_god_rays()`, `with_volumetric_settings(VolumetricSettings)`; getters/setters for color, radius, intensity, direction, angle, shadows (`casts_shadows`, `set_cast_shadows`), softness (`shadow_softness() -> f32`, `set_shadow_softness(size_px: f32)`, in pixels as `with_soft_shadows`), volumetric |
| `LightOccluder2D` (component) | `polygon(Vec<Vec2>)` (≥ 3 vertices, CCW), `chain(Vec<Vec2>)` (≥ 2 vertices: an open run of edges, wound as the polygon it is part of, each edge casting as it would there; chains that close a ring cast as that polygon), `rect(w, h)` (centred), `rect_offset(x, y, w, h)`, `circle(r, segments)`, `line(start, end, thickness)`, `vertices()`, `edges()`, `is_closed()` (false for a chain), `is_enabled()`, `set_enabled(bool)`, `with_enabled(bool)`, `self_shadow()`, `set_self_shadow(bool)`, `with_self_shadow(bool)` (off by default: on, every side casts, toward a light and away, so the near face governs and its own area is in its shadow). A tile map's are chains marked `MapOccluder` ([Tilemaps](#tilemaps)) |
| `AmbientLight { color, intensity }` | `new(Color, f32)`, `with_intensity(f32)`, presets `dark()`, `night()`, `moonlit()`, `indoor()`, `daylight()`, `sunset()`, `set_intensity`, `set_color`, `with_custom_intensity`, `effective_color()` |
| `Atmosphere { density, color, scattering, god_rays_* }` | `new(density, Color)`, presets `clear()`, `hazy()`, `dusty()`, `foggy()`, `smoky()`, `underwater()`, `with_god_rays()`, `with_god_rays_config(..)`, `with_god_rays_quality(GodRaysQuality)`, `with_density`, `with_color`, `with_scattering` |
| `GodRaysQuality { Low, Medium, High }` | `samples()`, `decay()` |
| `ShadowQuality { Low, Medium, High, Ultra }` (`Default` `High`) | `texels() -> u32` (256, 512, 1,024, 2,048: across each quarter of a light's shadow map); the player's setting, `Settings::shadow_quality` ([Config, settings and saves](#config-settings-and-saves)) |
| `VolumetricSettings { enabled, intensity, density, scattering, god_rays }` (per light) | `new(intensity, density, scattering)`, `enabled()`, `god_rays_only()`, `subtle_beam()`, `strong_beam()`, `flashlight()`, `with_god_rays`, `with_intensity`, `with_density`, `with_scattering` |

Lights and occluders sit at their entity's `GlobalTransform`. A light with a `RenderLayers` component on its entity (default `layer(0)`) lights only the cameras on those layers. Lights are batch quads with no cap; each lit camera has its own light buffers, so lights turn, zoom and shake with the camera, and each shadow darkens only its own light. Shadows come from per-light shadow maps in a shared atlas, drawn once a frame for the shadowed lights a lit camera sees and only when what they cast changed; a light inside a closed occluder is not shadowed by it; at most 512 lights a frame are shadowed (the nearest a lit camera's centre; one warning). Colours are sRGB: `Color::to_linear() -> [f32; 4]` is the linear-light, straight-alpha form the GPU gets. Light `intensity` above 1 shows (the look rolls it off); `Atmosphere::god_rays_threshold` is linear luminance (default 1.0). See `ryo-lighting`.

---

## The look

`Look` is a resource (`ctx.look()`): how the frame's linear HDR image becomes the picture, applied once per frame by the `tonemap` pass. `SceneCleanup::EFFECTS` resets it.

| Type | API |
|---|---|
| `Look { exposure: f32, tonemap: Tonemap, grading: ColorGrading, auto_exposure: Option<AutoExposure> }` | `Default`: exposure 0 EV (+1 doubles the light), `Tonemap::Soft { knee: 0.8, white: 4.0 }`, neutral grading, no auto-exposure. Order: exposure (plus the auto EV), white balance, contrast and saturation, the curve, lift/gamma/gain, then the LUT in sRGB |
| `enum Tonemap { Soft { knee, white }, Clamp, Aces, AgX }` | `apply(self, Vec3) -> Vec3` (linear in after exposure, linear display colour out). `Soft` is identity up to `knee`, then reaches 1 at `white`, keeping hues; `Clamp` clips at 1 (the pre-HDR look); `Aces` is contrasty; `AgX` is soft with hue-faithful highlights |
| `ColorGrading { temperature, tint, contrast, saturation, lift: Vec3, gamma: Vec3, gain: Vec3, lut: Option<ColorLut>, lut_strength }` | `Default`: 0, 0, 1, 1, `Vec3::ZERO`, `Vec3::ONE`, `Vec3::ONE`, none, 1.0. `temperature` and `tint` are -1 to 1; `contrast` pivots on mid-grey 0.18; `lift`, `gamma` and `gain` work per channel on the display colour |
| `ColorLut` | `from_sprite(&Handle<Image>, &Assets<Image>) -> Result<ColorLut, LutError>` (it holds the image), `size() -> u32`, `identity_strip(size: u32) -> Image` (N from 2 to 64). A strip is N tiles of N x N side by side (N*N wide, N high; 1024x32 for N = 32): tile `b` holds blue `b / (N - 1)`, red grows to the right and green downward. Looked up in sRGB; follows its image's reloads, keeping the last good strip |
| `enum LutError { WrongSize { width, height }, NotLoaded }` | the image is not a strip, or not loaded yet |
| `AutoExposure { min_ev, max_ev, compensation, speed_up, speed_down }` | `Default`: -4, 4, 0, 3, 1 (EV; speeds in EV per second, real time). Measured on the GPU from the scene (the average luminance without its darkest and brightest 5%) and brought to mid-grey; added to `exposure` |

```rust
ctx.look().exposure = 0.5;
ctx.look().tonemap = Tonemap::Aces;
```

---

## Assets

One registry for every asset type. `AssetServer` (a resource; systems take `Res<AssetServer>`) loads by asset path, relative to the project's `assets/` folder, through loaders chosen by extension, and each type's values are in its `Assets<T>` resource. Scenes reach both through `ctx.assets() -> AssetsView<'_>`, which borrows the world. Each frame the pump (`PreUpdate`) lands finished loads, sends `AssetEvent`s and frees assets whose last handle dropped. The guide is `ryo-assets`.

| `AssetsView` (`ctx.assets()`; also `HeadlessEngine::assets()`, `UiHarness::assets()`) | |
|---|---|
| `load<T: Asset>(&self, path: &str) -> Handle<T>` | queued on the worker threads (in the pump on the web, at most 4 ms a frame); returns at once. A bad path or an extension no loader takes gives a failed handle. One path (with one settings value) is one asset: asking again while it lives gives the same handle |
| `load_with<T: Asset, S: AssetSettings>(&self, path: &str, settings: S) -> Handle<T>` | with the loader's settings, part of the asset's key; settings of another type fail the load |
| `load_now<T: Asset>(&mut self, path: &str) -> Result<Handle<T>, Arc<AssetError>>` | at once on this thread, dependencies too, its `Added` or `Failed` sent at once; an asset already loaded comes back without a read |
| `load_now_with<T: Asset, S: AssetSettings>(&mut self, path: &str, settings: S) -> Result<Handle<T>, Arc<AssetError>>` | |
| `add<T: Asset>(&mut self, value: T) -> Handle<T>` | an asset made in code (its type is registered at the first add); never reloaded |
| `get<T: Asset>(&self, id: impl Into<AssetId<T>>) -> Option<&T>`, `get_mut<T: Asset>(&mut self, id) -> Option<&mut T>` | the value once loaded (`&handle` converts); `get_mut` moves its version on, so what was built from it is built again, and sends `Modified` at the next pump |
| `state<T: Asset>(&self, id) -> LoadState`, `is_ready<T: Asset>(&self, id) -> bool` | its own state; loaded with every dependency loaded too |
| `progress<I>(&self, ids: I) -> Progress` (`I: IntoIterator`, items `Into<UntypedAssetId>`), `stats(&self) -> AssetStats` | a group's progress, from its handles (`&handles`, `[&a, &b]`) or ids, untyped across types (an id neither loaded nor loading is not counted); counts per type |
| `server(&self) -> &AssetServer` | for `reload`, `path` and `dependency_state` |

| `AssetServer` (resource, cheap to clone) | |
|---|---|
| `load`, `load_with`, `state`, `is_ready`, `progress`, `stats` | as on `AssetsView` |
| `load_now<T: Asset>(&self, world: &mut World, path: &str) -> Result<Handle<T>, Arc<AssetError>>`, `load_now_with(..)` | with the world (exclusive systems, tests) |
| `dependency_state<T: Asset>(&self, id) -> LoadState` | `Loading` until every dependency is loaded; `Failed` with the first failed one's error, as an `AssetError::Dependency` |
| `reload(&self, path: &str)` | reload every asset loaded from `path` (each settings variant) on the workers: each keeps its id; a failure keeps the last good value (`ReloadFailed`) |
| `path<T: Asset>(&self, id) -> Option<AssetPath>` | the path it was loaded from |
| `new(impl AssetSource) -> AssetServer`, `register_asset::<T: Asset>(&self, &mut World)`, `register_loader(&self, impl AssetLoader)` | a registry of your own (tests); the engine makes the world's, with every engine type and loader registered |

- `Handle<T>` (`Clone`, `Eq`, `Hash`, `Debug`): `id() -> AssetId<T>`. The asset lives while any clone does; the pump frees it once the last one drops (`Removed`, if it was in its store; a failed load is freed with no event). Hold what a scene uses in the scene or its components, and what the whole game uses in a resource.
- `AssetId<T>` (`Copy`, `Eq`, `Hash`, `Debug`; `From<&Handle<T>>`): `index()`, `generation()`, `untyped() -> UntypedAssetId`. It names an asset without keeping it alive; a stale id finds nothing. `UntypedAssetId::typed::<T>() -> Option<AssetId<T>>`; `UntypedAssetId: From<AssetId<T>>`, `From<&Handle<T>>` (and from references to them).
- `Assets<T>` (resource: `Res<Assets<T>>`, `ResMut<Assets<T>>`): `add(T) -> Handle<T>`, `get(id) -> Option<&T>`, `get_mut(id) -> Option<&mut T>`, `version(id) -> Option<u64>` (moves on whenever the value changes), `len()`, `is_empty()`, `iter() -> impl Iterator<Item = (AssetId<T>, &T)>` (the engine's own assets count: the checker and the pixel theme's skins among the images, the built-in fonts among the fonts). A store comes only from the world (`ctx.assets()`, `ResMut<Assets<T>>`, `HeadlessEngine::assets()`, `testing::asset_world()`) and has no public constructor: a handle names a slot of the store that made it, and the renderer, the UI and `ctx.assets()` read the world's, so a handle from a store of its own would find nothing there, or another asset.
- `enum LoadState { NotLoaded, Loading, Loaded, Failed(Arc<AssetError>) }`: `is_loaded()`, `is_failed()`, `error() -> Option<&Arc<AssetError>>`. An asset added in code is `Loaded` while it lives.
- `Progress { loaded, failed, pending }` (`Copy`, `Eq`, `Default`): `is_done()` (nothing pending), `fraction()` (the share loaded or failed; 1 for an empty group). `AssetStats { types: Vec<TypeStats> }`, `TypeStats { name, loaded, loading, failed }`, one per registered type.
- `enum AssetEvent<T: Asset> { Added { id }, Modified { id }, Removed { id }, Failed { id, error: Arc<AssetError> }, ReloadFailed { id, error } }` (`Clone`, `Debug`): a message per asset type (`ctx.messages::<AssetEvent<Image>>()`, `MessageReader<AssetEvent<T>>`). The pump sends them in `PreUpdate`, `load_now` its own at once; an asset's events come in the order its changes happened, `Removed` last, and only for an asset that was `Added` (a load dropped before it landed, or a failed one, is freed with no event).
- `AssetPath` (`Clone`, `Eq`, `Hash`, `Ord`, `Display`): `parse(&str) -> Result<AssetPath, AssetError>` (`\` to `/`, `.` dropped, `..` resolved; absolute or leaving the root is `InvalidPath`), `as_str()`, `extensions()` (longest first: `"ui.ron"`, `"ron"`), `parent()`.
- The engine's types and loaders, in every registry: `Image` (`ImageLoader`: `png`, `jpg`, `jpeg`, `bmp`, `gif`, `webp`, `tga`, `tif`, `tiff`, `qoi`, `ico`, `svg`; settings `ImageSettings { svg: SvgSize }`, `enum SvgSize { Native (default), Scale(f32), Fit { width: u32, height: u32 } }`), `Shader` (`ShaderLoader`: `wgsl`), `Font` (`FontLoader`: `ttf`, `otf`, `ttc`; `FontSettings { face: u32 }`), `BitmapFont` (`BitmapFontLoader`: `bitmap.ron`), `UiFile` (`UiFileLoader`: `ui.ron`), `Theme` (`ThemeLoader`: `theme.ron`), `AudioClip` (`AudioClipLoader`: `wav`, `ogg`, `mp3`, `flac`; settings `AudioClipSettings { mode: ClipMode }`), `Sound` (`SoundLoader`: `sound.ron`), `Music` (`MusicLoader`: `music.ron`) and `Mixer` (`MixerLoader`: `mixer.ron`), see [Audio](#audio); `ParticleEffect` (`ParticleEffectLoader`: `particles.ron`), see [Particles](#particles); `SkeletonData` (`SkeletonLoader`: `skeleton.ron`), see [Skeletons](#skeletal-animation); `TileMap` (`TiledMapLoader`: `tmx`, `tmj`), `Tileset` (`TiledTilesetLoader`: `tsx`, `tsj`), `LdtkProject` (`LdtkLoader`: `ldtk`) and `TileWorld` (`TileWorldLoader`: `world`), see [Tilemaps](#tilemaps). A path's loader is the one taking its longest extension.
- A type of your own: `trait Asset: Send + Sync + 'static {}`, and for files a loader registered with `EngineBuilder::asset_loader(loader)`:

  ```rust
  pub trait AssetLoader: Send + Sync + 'static {
    type Asset: Asset;
    type Settings: AssetSettings; // `()` for none
    fn extensions(&self) -> &[&'static str]; // "level", "ui.ron"; a leading dot (".level") is dropped
    fn load(&self, bytes: Vec<u8>, settings: &Self::Settings, cx: &mut LoadContext<'_>) -> Result<Self::Asset, AssetError>;
    fn check(&self, asset: &Self::Asset, cx: &CheckContext<'_>) -> Result<(), AssetError> { Ok(()) } // main thread, once the dependencies load and whenever one reloads
    fn dependency_failed(&self, asset: &Self::Asset, error: &AssetError) -> Option<AssetError> { None } // a better error than the server's `Dependency`
  }
  ```

  `AssetSettings` is any `Default + Clone + Hash + Eq + Send + Sync + 'static` type. `LoadContext`: `path() -> &AssetPath`, `load::<T>(path) -> Result<Handle<T>, AssetError>`, `load_with::<T, S>(path, settings)`: a dependency, kept by keeping its handle in the asset; the asset is inserted once its dependencies are loaded. `CheckContext`: `path() -> &AssetPath` (to name the asset in its error: `AssetError::decode(cx.path(), ..)`), `world() -> &World` (the dependencies' stores). A panic in a loader fails its asset in native builds; the web build aborts on a panic, so return an error for a bad file.
- `trait AssetSource: Send + Sync + 'static { fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError>; fn exists(&self, path: &AssetPath) -> bool { /* reads it */ } fn watch(&self, changes: AssetChanges) -> Result<(), AssetError> { Ok(()) } }`: `exists` answers for an optional file the engine loads only if present (the mixer at `MIXER_PATH`), and the default reads the file; `watch` is asked once, when hot reload is on; keep `changes` (`Clone`, any thread) and call `changes.send(path: AssetPath) -> bool` for each path whose bytes changed (false once the server is gone). `DiskSource::new(root)` (`root()`; the engine's reads `assets/`), `MemorySource` (`new()`, `insert(path, bytes)`, `remove(path)`; clones share their files, and an `insert` over a path tells a watcher it changed). `EngineBuilder::asset_source(source)` reads from another source.
- `AssetError` (`#[non_exhaustive]`; `Display` is one line, the causes its `source()` chain): `NotFound { path }`, `Io { path, source: io::Error }` (`ErrorKind::Unsupported` on the web, which has no files; noted at debug level, not warned), `Decode { path, source }`, `NoLoader { path, extension }`, `Dependency { path, dependency, error: Arc<AssetError> }`, `Cycle { path, chain }`, `InvalidPath { path: String, reason }`; `AssetError::decode(&AssetPath, impl Into<Box<dyn Error + Send + Sync>>)` makes a `Decode` in a loader or its check; `report() -> String` is the error and every cause on one line, as the log prints it. Each failure is warned once in the log.
- Hot reload (`EngineBuilder::hot_reload`, default on in debug builds with a window): a file saved in `assets/` reloads under the same handle about 100 ms after its last write, and the assets that hold it are checked again; a reload that fails keeps the last good value.

---

## Window and cursor

- `WindowRequest { pub ui_scale: Option<Option<f32>> }` (resource, `ctx.window()`; its window size and display mode are the engine's, set from `Settings`): `new()`, `has_changes()`, `clear()`. Prefer `ctx.request_ui_scale`. The window's size and display mode are the player's settings: `ctx.settings().set_window_size(w, h)`, `set_display_mode(mode)`.
- `WindowInfo { scale_factor: f64, display_mode: DisplayMode }` (resource kept current by the engine).
- `CursorRequest { cursor, custom_cursor, visible, confined, locked }` (resource, `ctx.cursor()`): `has_changes()`. Prefer the `ctx.set_cursor*` helpers. The cursor you set (`ctx.set_cursor`, `ctx.set_custom_cursor`) stays until you set another; the engine shows it except over the developer console, where the console's cursor shows, and gives it back the frame the pointer leaves.
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
| `add_custom(name, Handle<Shader>) -> Result<(), EffectError>`, `add_custom_in(name, Handle<Shader>, EffectStage) -> Result<(), EffectError>`, `set_custom_param(name, param, value) -> bool`, `set_custom_enabled(name, bool) -> bool`, `remove_custom(name) -> bool`, `has_custom(name)` | game-written full-screen WGSL; `add_custom` runs on the display image, `add_custom_in` in the stage you pick. The effect holds its shader; one not loaded (or with no version that compiles and the GPU accepted) is skipped |
| `new()`, `update(dt)`, `time()`, `active_effects()`, `active_effects_in(EffectStage)`, `has_active_effects()`, `has_active_effects_in(EffectStage)`, `clear()` | |

`enum PostProcessEffect { Fade, ColorTint, Vignette, ScreenFlash, Crt, ChromaticAberration, GodRays, AmbientOcclusion, Bloom }` (what `active_effects` yields); `PostProcessEffect::stage() -> EffectStage`; `enum EffectError { DuplicateName(String) }`.

`enum EffectStage { Hdr, Display }`: `Hdr` runs on the scene image in linear light that may exceed 1, before the look (bloom, god rays, ambient occlusion); `Display` runs on the display image after it (fade, tint, vignette, flash, CRT, chromatic aberration). A custom effect's `scene(uv)` is linear in both; `Color` params stay sRGB bytes/255, so decode one (`srgb_to_linear`) before mixing it with `scene(uv)`.

Bloom's `threshold` is linear luminance, 0 or more and unbounded above (default 1.0, white): only light brighter than white blooms, so lit highlights, emission and intensity above 1 feed it. God rays' threshold is the same measure.

---

## Trails and lines

- `with_emission(f32)` (default 1) on `Trail2D`, `Line2D`, `ParticleEffect` and `Renderable` multiplies the colour's light: above 1 it glows past white and feeds bloom. `Trail2D` and `Line2D` read it with `emission()`; `Renderable` and `ParticleEffect` have a public `emission` field.
- `Trail2D` (component, on an entity with a `Transform`): records the entity's world position every fixed step (points are world space). Constructors `new(Color, width)`, `bullet(Color)`, `swipe(Color, w)`, `particle(Color)`, `laser(Color, w)`, `fading_laser(Color, w, fade_time)`, `lightning(Color, w)`, `fire(Color, w)`, `energy(Color, w)`; builders `with_length(max_points)` (≤ 128), `with_end_width`, `with_taper(ratio)`, `with_fade(bool)`, `with_end_color`, `with_min_distance`, `with_emit_interval`, `with_lifetime(secs)`, `with_fade_distance(px)`, `with_texture(Handle<Image>)`, `with_texture_mode(TextureMode)`, `with_tile_length`, `with_style(TrailStyle)`, `with_effect(TrailEffect)`, `with_cap(LineCap)`, `with_join(LineJoin)`, `with_blend(BlendMode)`, `with_filter(Filter)` (textured trails), `with_lighting(bool)`, `with_z_order(i32)`; control `set_enabled`, `set_visible`, `set_recording`, `clear`, `push_point(x, y)`, `push_point_back`, `populate_line(..)`, `update(x, y, dt)`, plus getters/setters for each setting.
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

Shapes added in a fixed step (`update`, fixed systems) are drawn until the next step begins; shapes added elsewhere for one frame. The engine's collider, velocity, origin and skeleton (`debug:skeletons`) overlays use it.

---

## Skeletal animation

A rig (`SkeletonData`, an asset: bones, slots, attachments, skins, animations and events) is read from a `.skeleton.ron` file or built in code; an entity with a `Skeleton` shows one, posed by its `SkeletonAnimation` in each fixed step (`FixedSet::Skeletons`: after propagation, before particles) and drawn by its `Renderable::skeleton()` as a sprite is. A game calls nothing each frame. Coordinates are y down and positive rotation clockwise; angles are radians in code and degrees in files. The guide, with the file format, every rule and the limits, is `ryo-skeleton`.

| `Skeleton` (component, requires `Transform` and `Interpolated`) | |
|---|---|
| `new(data: Handle<SkeletonData>) -> Self` | shows `data` where its entity stands, in its setup pose and default skin. Until the data has loaded it has no bones and waits; what is set on it meanwhile is kept by name. Putting it on an entity adds `Interpolated`, so the entity, its bones and its sockets are drawn between fixed steps; remove `Interpolated` afterwards for step-rate drawing |
| `data(&self) -> &Handle<SkeletonData>` | the data it shows |
| `set_skin(&mut self, name: &str)`, `set_skins(&mut self, names: &[&str])` | wears one skin, or several as one, over the default skin (an empty list: the default alone). An attachment is looked up in the last skin worn that has it, then in the default. A name the data lacks warns once per skeleton per name and is left out |
| `skins(&self) -> impl Iterator<Item = &str>` | the skins worn, in order, without the default; a `set_skins` reads back at once |
| `set_attachment(&mut self, slot: &str, attachment: Option<&str>)` | shows an attachment in a slot whatever animations key there (`None` hides the slot), until cleared |
| `clear_attachment(&mut self, slot: &str)` | hands the slot back to the animations' keys, else its setup attachment |
| `attachment(&self, slot: &str) -> Option<&str>` | the name the slot shows as the last step left it (the game's, else the animations' key, else the setup attachment). `None` for a hidden slot, a name no skin worn has in the slot, a slot the data lacks, and before the data has loaded |
| `override_bone(&mut self, bone: &str, o: BoneOverride)`, `clear_override(&mut self, bone: &str)` | sets a bone after animation, every step until cleared; its children follow |
| `bone_world(&self, name: &str) -> Option<Affine2>` | a bone's world transform as the last step left it: its origin and its axes |
| `point(&self, name: &str) -> Option<(Vec2, f32)>` | a point attachment a slot shows, in the world: its place and its direction in radians |
| `bounding_box(&self, name: &str, out: &mut Vec<Vec2>) -> bool` | fills `out` with the world corners of a bounding box a slot shows; false, leaving `out` empty, when none shows (an animation switches a hit box by keying its slot) |

- What a game sets shows from the skeleton's next step. What it reads in a scene's `update` is as the last step left it, one step behind that step's motion; a `BoneSocket`, or a system added with `EngineBuilder::add_systems` and ordered `.after(FixedSet::Skeletons)`, is in step.
- `BoneOverride { rotation: Option<f32>, translation: Option<Vec2>, scale: Option<Vec2>, space: BoneSpace, mix: f32 }` (`Default`: nothing set, `Local`, mix 1): each value given replaces the animated one by `mix` (0 to 1), a rotation the short way round. `enum BoneSpace { Local (default), World }`: in `World` a rotation is the way the bone's x axis points and a translation is where its origin is; `scale` is always the bone's own. A value that is not finite is ignored, with one warning per skeleton.
- `BoneSocket { bone: String, offset: Transform, inherit_scale: bool }` (component, requires `Transform`; on a child of the skeleton's entity): `new(bone)` (at the bone's origin, taking its scale), `with_offset(Transform)`, `without_scale()`. The engine sets the entity's `Transform` to the bone's followed by the offset in the skeletons' step, and places everything below it in the same step. A bone the skeleton lacks warns once for that name. A skew in the bone is dropped; a collider on a socket follows its bone one physics step late.

| `SkeletonAnimation` (component) | |
|---|---|
| `new() -> Self` (`Default`) | no track playing; without the component a skeleton holds its setup pose |
| `time_scale: f32` (public field, 1) | multiplies every track's time; a negative or non-finite scale holds the skeleton still |
| `MAX_TRACKS: usize` (64), `EMPTY: &'static str` (`"<empty>"`) | tracks are 0 to 63; the empty animation's name, as its track events carry it |
| `set_animation(&mut self, track: usize, name: &str, looping: bool) -> Option<SkeletonTrackEntryId>` | plays `name` at once: the entry it replaces mixes out over the new entry's `mix_duration`, and the queue is dropped. On an empty track it shows at once. `None` for a name the data lacks (warned once per skeleton per name) and a track past 63 |
| `add_animation(&mut self, track: usize, name: &str, looping: bool, delay: f32) -> Option<SkeletonTrackEntryId>` | queues `name`: it starts `delay` seconds into the entry before it, or with `delay <= 0` at that entry's end plus `delay` (a looping entry's end is the end of the loop it is in); on an empty track, after `delay` seconds |
| `set_empty(&mut self, track: usize, mix: f32)` | fades the track out over `mix` seconds, then clears it; an entry queued after it mixes in from nothing (a layer fading in) |
| `clear(&mut self, track: usize)` | stops the track at once and drops its queue |
| `entry(&self, id: SkeletonTrackEntryId) -> Option<&SkeletonTrackEntry>`, `entry_mut(&mut self, id) -> Option<&mut SkeletonTrackEntry>` | the entry a handle names (playing, mixing out, queued), until its `Dispose` |
| `current(&self, track: usize) -> Option<SkeletonTrackEntryId>` | the entry playing (the one mixing in, during a mix); never the empty animation's |
| `animation(&self, track: usize) -> Option<&str>` | the name of the animation playing; `None` for an empty track and one `set_empty` is fading out. An animation set reads back at once |
| `mix_progress(&self, track: usize) -> Option<f32>` | how far the playing entry has mixed in, 0 up to 1; `None` when the track is not mixing |

- `SkeletonTrackEntry` (public fields, changed through `entry_mut`): `speed: f32` (1), `alpha: f32` (1, how much it applies over what is under it), `mix_duration: f32` (the data's mix for the pair, else its default mix), `mix_blend: SkeletonMixBlend` (`Replace`), `event_threshold: f32` (0: an entry mixing out fires no event keys), `reverse: bool`, `hold_previous: bool` (the entry it replaced holds its time while this one mixes in), `looping: bool`, `delay: f32`, `start: f32` (0: seconds into the animation at which it begins, read at its first step; the event keys at or before a start past 0 do not fire, and an entry that is not looping started at or past its end holds its last frame and sends no `Complete`). Methods: `animation() -> &str`, `track_time() -> f32` (seconds played, counting every loop, from its start; before its first step, its start), `is_complete() -> bool`.
- `enum SkeletonMixBlend { Replace, Add }`: `Replace` moves the pose toward the animation by the entry's alpha; `Add` adds the animation's change from the setup pose. `SkeletonTrackEntryId` (`Copy`, `Eq`, `Hash`).
- `enum SkeletonTrackEventKind { Start, Interrupt, End, Complete, Dispose }`: it became its track's playing entry; another entry replaced it; it will never be applied again; it passed its animation's end while playing (each loop when looping; never while mixing out, never the empty animation's); it is gone and its handle finds nothing.
- `SkeletonEvent` and `SkeletonTrackEvent`: see [Messages](#messages). The engine plays no sound for an event: `audio` (`SkeletonEventAudio { path: Arc<str>, volume: f32, balance: f32 }`) is the game's to play.

| `SkeletonData` (asset; `SkeletonLoader` reads `skeleton.ron`) | |
|---|---|
| `builder() -> SkeletonDataBuilder` | builds one in code |
| `DEFAULT_SKIN: &'static str` (`"default"`) | the skin every skeleton has |
| `bones() -> &[BoneData]`, `slots() -> &[SlotData]`, `regions() -> &[SkeletonRegion]`, `events() -> &[SkeletonEventData]`, `animations() -> &[SkeletonClip]` | bones parents first, slots in setup draw order |
| `skins() -> &[SkeletonSkin]`, `default_skin() -> &SkeletonSkin`, `skin(name: &str) -> Option<&SkeletonSkin>` | the default skin first |
| `bone(name: &str) -> Option<BoneId>`, `slot(name: &str) -> Option<SlotId>`, `animation(name: &str) -> Option<&SkeletonClip>` | by name |
| `mix(from: &str, to: &str) -> f32`, `default_mix() -> f32` | the seconds a crossfade takes: the pair's own, else the default |

| `SkeletonDataBuilder` (each call returns the builder) | |
|---|---|
| `bone(name, parent: Option<&str>, BoneSetup)` | a bone under a bone added before it, or a root |
| `slot(name, bone: &str, SlotSetup)` | a slot on a bone, drawn over the slots already added |
| `region(SkeletonRegion)`, `event(SkeletonEventData)`, `animation(SkeletonClip)` | region attachments and event keys index regions and events in the order added |
| `skin(name)`, `attachment(skin: &str, slot: &str, name, SkeletonAttachment)` | declares a skin (the default always exists); puts an attachment in a skin's slot under a name |
| `mix(from: &str, to: &str, seconds: f32)`, `default_mix(seconds: f32)` | mixes |
| `bone_id(&self, name) -> Option<BoneId>`, `slot_id(&self, name) -> Option<SlotId>`, `region_index(&self, name) -> Option<usize>`, `event_index(&self, name) -> Option<usize>` | the ids and indices animations and attachments use |
| `build(self) -> Result<SkeletonData, SkeletonError>` | checks everything and names the first thing wrong: `SkeletonError { item: String, problem: String }` (`Display`, `Error`) |

- `BoneSetup { length, x, y, rotation, scale_x, scale_y, shear_x, shear_y: f32, inherit: BoneInherit }` (`Default`: at its parent's origin, unit scale, no length); `BoneData` has the same fields with `name: Arc<str>` and `parent: Option<BoneId>`. `enum BoneInherit { Normal (default), OnlyTranslation, NoRotationOrReflection, NoScale, NoScaleOrReflection }`. `BoneId(pub u8)` and `SlotId(pub u8)`: `index() -> usize`; a skeleton has at most 255 of each.
- `SlotSetup { color: Color, attachment: Option<Arc<str>>, blend: SlotBlend }` (`Default`: white, no attachment, `Normal`); `SlotData` has the same fields with `name` and `bone: BoneId`. `enum SlotBlend { Normal (default), Additive, Multiply, Screen }`.
- `SkeletonRegion { name: Arc<str>, image: Handle<Image>, x: u32, y: u32, width: u32, height: u32 }`: a named pixel rectangle of an image.
- `enum SkeletonAttachment { Region(RegionAttachment), Point(PointAttachment), BoundingBox(BoundingBoxAttachment) }`. `RegionAttachment { region: usize, x, y, rotation, scale_x, scale_y, width, height: f32, color: Color }`: `new(region, width, height)` (centred on the bone, white). `PointAttachment { x, y, rotation: f32 }` (`Default`). `BoundingBoxAttachment { vertices: Vec<Vec2> }` (at least three).
- `SkeletonSkin { name: Arc<str> }`: `attachment(slot: SlotId, name: &str) -> Option<&SkeletonAttachment>`, `attachments() -> impl Iterator<Item = (SlotId, &Arc<str>, &SkeletonAttachment)>`.
- `SkeletonClip { name: Arc<str>, duration: f32, timelines: Vec<SkeletonTimeline> }`. `enum SkeletonTimeline`, a value key being `(time, value, SkeletonCurve)`: `Rotate { bone: BoneId, frames }` (`f32` radians), `Translate`, `Scale`, `Shear { bone, frames }` (`Vec2`), `TranslateX`, `TranslateY`, `ScaleX`, `ScaleY`, `ShearX`, `ShearY { bone, frames }` (`f32`), `Attachment { slot: SlotId, frames: Vec<(f32, Option<Arc<str>>)> }`, `Color { slot, frames }` (`Color`), `DrawOrder { frames: Vec<(f32, Option<Vec<SlotId>>)> }`, `Event { frames: Vec<(f32, usize, SkeletonEventValues)> }`; `kind() -> &'static str`. Bone keys are changes from the setup pose (scale a factor of it).
- `enum SkeletonCurve { Linear (default), Stepped, Bezier { cx1, cy1, cx2, cy2 } }` (`cx1` and `cx2` within 0 to 1): `value(t: f32) -> f32`.
- `SkeletonEventData { name: Arc<str>, int: i32, float: f32, string: Arc<str>, audio: Option<SkeletonEventAudio> }`: `new(name)`. `SkeletonEventValues { int: Option<i32>, float: Option<f32>, string: Option<Arc<str>>, volume: Option<f32>, balance: Option<f32> }` (`Default`): what an event key overrides.
- The console command `debug:skeletons` (a development build) draws every skeleton's bones, region outlines, bounding boxes and points.

---

## Particles

An effect (`ParticleEffect`, an asset) says how a kind of particle is born, moves, looks and ends; an emitter (`ParticleEmitter`, a component) places one in the world. The engine runs and draws them: game-time emitters in `FixedSet::Particles`, unscaled ones in `PostUpdateSet::Extract`, each drawn as one run. A game calls nothing each frame. Angles in effects are degrees.

| `ParticlesView` (`ctx.particles()`; `ParticlesView::new(&mut World)` in an exclusive system) | |
|---|---|
| `play_at(&mut self, effect: &Handle<ParticleEffect>, position: Vec2) -> Entity` | an emitter entity that plays one cycle of `effect`, even a looping one, and despawns when its last particle dies, sending `EmitterFinished` first |
| `count(&self) -> u32`, `budget(&self) -> u32` | live particles over every emitter; the most allowed at once |
| `clear(&mut self)` | removes every live particle at once; the emitters carry on |

- `Particles` (a `SystemParam`: `mut particles: Particles`): `play_at(&mut self, effect, position) -> Entity` (spawned through the system's commands, the entity reserved at once), `count()`, `budget()`.
- `EngineBuilder::particle_budget(max: u32)` (default `DEFAULT_PARTICLE_BUDGET: u32 = 20_000`): a spawn past it is dropped, with one warning per effect; live particles are never cut short.

| `ParticleEmitter` (component, requires `Transform`) | |
|---|---|
| `new(effect: Handle<ParticleEffect>)` | plays `effect` at its entity's `GlobalTransform` (lifted on a floor); public field `effect` |
| `with_seed(u64)`, `with_z(i32)` | its random stream's seed, over the effect's; its draw order, over the effect's `z` |
| `with_speed(f32)`, `set_speed(&mut self, f32)` | its time as a multiple of game time, or real time when unscaled (0 to 100; not a number or negative stops it) |
| `unscaled()` | runs on real time through a paused clock, hitstop, slow motion and held scenes (at most 8 steps a frame) |
| `stopped()` | emits nothing until `play`, and reports no finish before it has played |
| `once()` | plays one cycle, even of a looping effect |
| `despawn_when_finished()` | despawns its entity, with its children, in the step after it finishes |
| `play(&mut self)` | a stopped emitter resumes where it stopped; one whose one cycle ended plays it again from the start; arms `EmitterFinished` |
| `stop(&mut self)` | stops emitting; live particles carry on |
| `clear(&mut self)` | removes its live particles in its next step |
| `restart(&mut self)` | removes them and starts its cycle over at 0 (the start delay skipped), every burst due again; arms `EmitterFinished` |
| `burst(&mut self, count: u32)` | adds `count` particles in its next step, whatever its effect emits and whether or not it plays, within `max_particles` and the budget. A `stopped()` emitter never reports `EmitterFinished` nor despawns: only `play` or `restart` arm it |
| `is_playing() -> bool`, `is_finished() -> bool`, `is_unscaled() -> bool`, `alive() -> u32`, `speed() -> f32`, `z() -> Option<i32>` | finished: not playing, no live particles and no burst to add; `alive` as the last step left it |

`ctx.skip_interpolation(entity)` also marks a `ParticleEmitter` on the entity, `Interpolated` or not: its next step spawns at the new place only, so a teleport never streaks. Despawning an emitter, or taking the component off, removes its particles. A `Disabled` emitter is frozen and hidden. A game-time emitter steps while its scene updates, so not under a scene that stops it; a scene transition doesn't stop it; an emitter added outside every scene is held while any scene is held.

| `ParticleEffect` (asset; public fields, `.particles.ron` files through `ParticleEffectLoader`) | Default | |
|---|---|---|
| `new(image: Handle<Image>)` | | presets below |
| `with_rate(f32)`, `with_rate_per_distance(f32)` | 10, 0 | particles per second; per pixel the emitter moves (a `World` effect's spread along the move, none across a teleport) |
| `with_burst(Burst)` | none | adds to `bursts: Vec<Burst>` |
| `with_duration(f32)`, `with_looping(bool)`, `with_start_delay(f32)` | 1, `true`, 0 | one cycle, in seconds |
| `with_prewarm(bool)` | `false` | starts a looping effect as if it had played a cycle (at most 10 s, run in its first step) |
| `with_max_particles(u32)` | 1000 | per emitter |
| `with_shape(EmitShape)` | `Point` | where particles are born; turns and scales with the emitter |
| `with_direction(direction_deg: f32, spread_deg: f32)`, `outward()` | 0, 360 | 0 is right, 90 is down; `outward` travels away from the shape's centre |
| `with_lifetime(impl Into<ValueRange>)`, `with_speed(..)`, `with_size(..)` | (1, 2), (0, 50), (8, 16) | |
| `with_rotation(..)`, `with_angular_velocity(..)` | 0, 0 | degrees, degrees per second (fields `rotation_deg`, `angular_velocity_deg`) |
| `with_inherit_velocity(f32)` | 0 | the share of the emitter's velocity a `World` effect's particle starts with |
| `with_color(impl Into<ColorRange>)` | white | a `Color`, or `(Color, Color)` for a random mix |
| `with_color_over_life(Gradient)` | white to transparent | multiplies the start colour |
| `with_size_over_life(Curve)`, `with_speed_over_life(Curve)` | 1, 1 | multiply the start size; the velocity by the curve's change each step |
| `with_gravity(x: f32, y: f32)`, `with_drag(f32)` | 0, 0 | pixels per second squared; fraction of speed lost per second |
| `with_radial_acceleration(impl Into<ValueRange>)`, `with_tangential_acceleration(..)` | 0, 0 | about the emitter, one value per particle |
| `with_noise(NoiseForce)`, `with_space(Space)` | none, `World` | a `Local` effect ignores `inherit_velocity`, collisions and attractors |
| `with_blend(BlendMode)`, `with_filter(Filter)`, `with_lighting(bool)`, `with_emission(f32)` | `Alpha`, `Default`, `true`, 1 | field `lit` for the third |
| `with_effect(SpriteEffect)`, `with_z(i32)`, `with_draw_order(DrawOrder)`, `with_frames(Flipbook)`, `with_align(ParticleAlign)` | none, 0, `NewestOnTop`, none, `None` | |
| `with_collision(CollisionConfig)`, `on_death(SubEmitter)`, `on_collision(SubEmitter)` | none | |
| `with_layer(u32)`, `with_seed(u64)` | 0, none | the bits attractors' layer masks test (a camera's layers are the entity's `RenderLayers`); a fixed stream |

Presets (each `(Handle<Image>) -> ParticleEffect`): `fire`, `flame_small`, `smoke`, `smoke_thick`, `magic`, `magic_heal`, `rain`, `snow`, `sparks`, `dust`, `confetti`, `bubbles`, `comet_trail`, `exhaust`; `explosion(image, count: u32)`, `explosion_debris(image, count: u32)`. `sparks`, `dust`, `confetti` and the explosions are one burst, played once.

- `enum EmitShape { Point, Circle { radius, arc_deg, thickness }, Rect { size: Vec2, thickness }, Line { length } }` (`thickness` 0 is the edge only, 1 the whole area).
- `Burst { time, count: ValueRange, cycles, interval }`: `Burst::at(time: f32, count: impl Into<ValueRange>)`, `with_cycles(cycles: u32, interval: f32)` (0 cycles repeats until the emitter stops; a burst fires only within its cycle).
- `ValueRange { min, max }`: `new(min, max)`, `constant(v)`; `From<f32>`, `From<(f32, f32)>`; a min above its max samples the min. `ColorRange { a, b }`: `new(a, b)`, `constant(c)`; `From<Color>`, `From<(Color, Color)>`.
- `Curve`: `new(impl IntoIterator<Item = (f32, f32)>)`, `eased(impl IntoIterator<Item = (f32, f32, Ease)>)`, `constant(v)`, `keys() -> &[CurveKey]`, `sample(t) -> f32`. `CurveKey { t, value, ease }`. `Gradient`: `new(impl IntoIterator<Item = (f32, Color)>)`, `constant(c)`, `keys() -> &[GradientKey]`, `sample(t) -> Color`. `GradientKey { t, color }`.
- `Flipbook { columns, rows, count, mode: FrameMode, random_start }`: `new(columns: u32, rows: u32)`, `with_count(u32)`, `at_fps(f32)`, `random_start()`. `enum FrameMode { OverLife, Fps(f32) }`.
- `enum ParticleAlign { None, Velocity { stretch: f32 } }` (the prelude's name for `particles::Align`; a length of `size + speed × stretch`). `enum DrawOrder { NewestOnTop, OldestOnTop }`. `enum Space { World, Local }`. `NoiseForce { strength, frequency, speed }`: `new(strength, frequency)`, `with_speed(f32)`.
- `SubEmitter { effect: Handle<ParticleEffect>, count: ValueRange, inherit_velocity: f32 }`: `new(effect, count: impl Into<ValueRange>)`, `with_inherit_velocity(f32)`. `on_death` plays where a particle dies, `on_collision` on any contact that would send a `ParticleCollision`; one level deep.
- `ParticleAttractor { attractor_type, layer_mask, enabled }` (component): `new(AttractorType)`, `point(strength, radius)`, `gravity_well(strength, radius)`, `line(x0, y0, x1, y1, strength, radius)`, `area(w, h, fx, fy)`, `vortex(strength, radius)`, `spiral(strength, radius, inward_pull)`, `turbulence(strength, scale, speed)`, `with_layer_mask(u32)`, `with_enabled(bool)`. `enum AttractorType { Point { strength, radius, falloff }, Line { .. }, Area { .. }, Vortex { .. }, Turbulence { .. } }`.
- `CollisionConfig { radius, collision_mask, response, send_events }`: `new(radius)`, `with_mask(u32)`, `with_response(CollisionResponse)`, `with_events()`, `die_on_hit(r)`, `bouncy(r, restitution)`, `sticky(r)`. `enum CollisionResponse { Die, Bounce { .. }, Stick, Event }` (`bounce(restitution)`, `restitution()`). Particles collide with `Collider` entities.
- `ParticleCollision { emitter_entity, collider_entity, position_x, position_y, normal_x, normal_y, velocity_x, velocity_y }` is a message (`ctx.messages::<ParticleCollision>()`); a bounce writes one only above `MIN_BOUNCE_EVENT_SPEED` (20 px/s).
- `EmitterFinished { entity }` is a message, sent once per finish in the step the emitter finishes (`despawn_when_finished` despawns in the next; an unscaled emitter can do both in one frame).

See `ryo-particles`.

---

## Tilemaps

Tiled and LDtk levels as registry assets, spawned by a `MapRoot` entity: tile layers drawn in cached 32 x 32-cell chunks through the world batch, colliding and casting 2D shadows from their tiles' shapes, objects and class tiles as entities, tiles read and edited through `ctx.maps()` with Wang terrain, worlds of levels and hot reload. All in the prelude; the guide is `ryo-tilemap`.

**Assets and loaders** (registered in every registry):

| Asset | Loader, files | |
|---|---|---|
| `TileMap { size: UVec2 (a bounded map's cells; zero for an infinite map), tile_size: UVec2, shape: GridShape (default orthogonal), infinite: bool, tilesets: Vec<TilesetSource>, layers: Vec<MapLayer>, class: Option<String>, background: Option<Color>, parallax_origin: Vec2, properties: Properties }` | `TiledMapLoader` (`default()`, `with_project(path)`): `tmx`, `tmj` | `new(size, tile_size)` (cells, a cell's pixels), `infinite(tile_size)` (an empty infinite map whose tile layers are unbounded and grow where a tile is written), `with_shape(GridShape) -> TileMap` (the tile layers that had the map's grid move onto the new one), `grid() -> MapGrid` (`MapGrid::for_map` of its shape, cell, size and `infinite`; each tile layer copies it as it is added), `bounds() -> CellRect` (a bounded map's (0, 0) to its size; an infinite map's the smallest rectangle holding every cell of its tile layers with a tile, empty with none), `add_tileset(TilesetSource) -> u16` (at most 256), `add_tile_layer(&str) -> usize` (unbounded on an infinite map; colliding on every grid), `add_object_layer(&str) -> usize`, `layer_index(&str) -> Option<usize>`, `tiles(layer) -> Option<&TileGrid>`, `tiles_mut(layer) -> Option<&mut TileGrid>`. Tilesets and images are dependencies. A Tiled infinite map loads with `infinite` true and every tile layer an unbounded `TileGrid` holding its chunks where Tiled put them |
| `Tileset { name, class: Option<String>, tile_size: UVec2, columns, tile_count, spacing, margin: u32, tile_offset: IVec2, image: TilesetImage, transparent: Option<Color>, wang_sets: Vec<WangSet>, transformations: TileTransformations, properties }` | `TiledTilesetLoader` (`default()`, `with_project(path)`): `tsx`, `tsj` | `new(name, tile_size, TilesetImage)`, `tile(id) -> Option<&TileData>`, `insert_tile(id, TileData)`, `wang_set(&str) -> Option<&WangSet>`, `tile_rect(id) -> Option<URect>` (its texels in the atlas) |
| `LdtkProject { levels: Vec<LdtkLevel>, worlds: Vec<WorldLayout>, tilesets: Vec<Arc<Tileset>> }` | `LdtkLoader`: `ldtk` (its `.ldtkl` level files read with it) | `level(name_or_iid) -> Option<&LdtkLevel>`; LDtk 1.5's format, a newer one fails |
| `TileWorld { layout: WorldLayout }` | `TileWorldLoader`: `world` | `maps() -> &[Handle<TileMap>]`, `map(level) -> Option<&Handle<TileMap>>`; its maps are dependencies |

- `TilesetSource::{Asset(Handle<Tileset>), Embedded(Arc<Tileset>)}`; `TilesetImage::{Atlas { image: Handle<Image>, size: UVec2 }, Collection}` (a collection's tiles have their own `TileData::image`).
- `MapLayer { info: LayerInfo, kind: LayerKind }`; `LayerKind::{Tiles { tiles: TileGrid, intgrid: Option<IntGrid>, grid: MapGrid }, Objects(Vec<MapObjectData>), Image(ImageLayer)}` (`grid` is the map's `grid()` unless the file gives the layer a grid of its own, as LDtk does: a square one of the layer's cell size).
- `LayerInfo` (also a component on each spawned layer entity) `{ id: u32, name, class, offset: Vec2, floor: u8, opacity: f32, tint: Color, parallax: Vec2, visible: bool, blend: BlendMode, filter: Filter, collides: bool, occludes: Option<bool> (None: as collides), self_shadow: bool, y_sorted: bool, lit: bool, part_of: Option<u32>, properties }`: `new(id, name)` (visible, opaque, lit, colliding, on the ground floor). Tiled fills `collides`, `occludes`, `self_shadow`, `y_sorted` and `lit` from the bool layer properties `collides` (default true for tile layers), `occludes`, `self_shadow` (default false; of another type it warns), `y_sort` and `lit` (default true), a tile or object layer's `filter` from the string layer property `filter` (`linear` or `nearest`; left out, `Filter::Default`, the target's), and `floor` (the storey: 0 the ground floor) from the int layer property `floor`, which a group passes to its layers that set none (a whole number from 0 to 255, else the map is refused, naming the layer); LDtk's IntGrid layers collide, its other layers do not, and LDtk maps have no floors. A layer on a floor above the ground stands in ground space, at its offset less its floor's height (`MapFloors`), read as the map lands.
- `ImageLayer { image: Option<Handle<Image>>, repeat_x: bool, repeat_y: bool, transparent: Option<Color> }` (also a component on an image layer's entity; `None` for a layer given no picture, which draws nothing; `transparent`, Tiled's transparent colour, draws clear). `IntGrid { size, values: Vec<i32>, names }`: `get(IVec2) -> Option<i32>` (`None` for 0 or outside).
- `MapObjectData { id: String, name, class, position: Vec2 (the rectangle's top-left in map pixels), size: Vec2, rotation: f32 (radians, clockwise on screen, about position), shape: ObjectShape, tile: Option<Tile>, visible: bool, properties }`; `ObjectShape::{Rect, Ellipse, Point, Capsule, Polygon(Vec<Vec2>), Polyline(Vec<Vec2>), Text(String)}`.
- `LdtkLevel { identifier, iid, world: Option<String>, position: IVec2, size: UVec2, depth: i32, fields: Properties, neighbours: Vec<Neighbour>, map: TileMap }`. A layer whose tiles stack, sit off the grid or fade loads with sub-layers `Name#1`, `Name#2`... listed after the editor's layers, each with `LayerInfo::part_of` naming its layer: parts of that layer, drawn with its settings, at its z, the IntGrid and collision its own.
- `WorldLayout { name, levels: Vec<WorldLevel> }`: `levels_in(Rect, depth: i32) -> impl Iterator<Item = (usize, &WorldLevel)>` (the levels at that world depth overlapping the area; touching an edge does not count). `WorldLevel { name, position: Vec2 (its rectangle's top-left), size: Vec2, origin: Vec2 (where the map's pixel (0, 0) sits, where a spawned root's `Transform` goes; a Tiled world's level is placed there by its x and y, its rectangle its cells' box on its grid from there: `position` is `origin` for a bounded map unless an oblique skew is negative, and a Tiled infinite map's tiles' bounds reach left and up of it), depth: i32 (LDtk's worldDepth, 0 for Tiled), neighbours: Vec<Neighbour> }`; `Neighbour { level: usize, direction: NeighbourDirection }`; `enum NeighbourDirection { North, South, East, West, NorthEast, NorthWest, SouthEast, SouthWest, Above, Below, Overlap }`.

**Cells and tiles:**

- `TileGrid` (4 bytes a cell, in chunks of `CHUNK_SIZE` = 32 cells a side keyed by signed chunk coordinate, each made when a tile is first written in it, or by `mark_all_dirty`, which a landing calls: a spawned bounded layer has every chunk of its bounds; a chunk stays once made, emptied or not, so a huge bounded map built in code is costly to land and keep, where an unbounded grid costs only the chunks written in): `new(size: UVec2)` (bounded: the cells from (0, 0) to `size`), `unbounded()` (any cell, negative ones included), `bounds() -> Option<CellRect>` (`None` when unbounded), `extent() -> CellRect` (the bounds, or the smallest rectangle holding all of an unbounded grid's chunks, and whatever lies between them), `get(IVec2) -> Option<Tile>`, `set(IVec2, Option<Tile>) -> bool` (false outside a bounded grid, unchanged, or a tile that does not fit a cell), `fill(min, max, Option<Tile>) -> usize` (both corners included; clearing reads only the chunks there are; a tile fill that would make more than 131,072 chunks, the 2^27 cells a map may hold, changes nothing and returns 0), `clear()`, `chunk_coords() -> impl Iterator<Item = IVec2>` (the chunks there are, by row then column; a chunk stays once made; walk these, not the extent, to read an unbounded grid), `chunk_of(IVec2) -> IVec2` (a cell's chunk, rounding toward negative infinity: cell -1 is in chunk -1), `chunk_version(IVec2) -> u64` and `version() -> u64` (move on with every change, never repeated across grids; 0 for a chunk never changed), `mark_all_dirty()`, `drain_dirty(&mut Vec<IVec2>)` (the chunks changed since the last drain, by row; one reader only).
- `MapGrid { shape: GridShape, cell: UVec2 (a cell's box: Tiled's tilewidth and tileheight), origin: Vec2 (where the shape's (0, 0) sits in the layer's pixels) }` (`Copy`, `PartialEq`): where a layer's cells are in its pixels (y down), and the board questions on every grid. `new(GridShape, cell: UVec2)` (origin zero), `with_origin(Vec2) -> MapGrid`, `for_map(shape, cell, size: UVec2, infinite: bool) -> MapGrid` (Tiled's origin: on a bounded isometric map the map's height times half a cell across, rounded down; zero for every other map), `cell_center(IVec2) -> Vec2`, `cell_outline(IVec2) -> CellOutline` (clockwise on screen from its top-most point), `cell_bounds(IVec2) -> Rect`, `tile_anchor(IVec2) -> Vec2` (where a tile image's bottom-left corner goes, before its tileset's offset), `cell_at(Vec2) -> IVec2` (the cell whose outline holds the point; an edge or corner cells share goes to one of them), `neighbours(IVec2) -> impl ExactSizeIterator<Item = IVec2>` (4, or 6 on hex maps, clockwise from the top), `distance(a, b) -> u32` (steps through neighbours), `line(a, b) -> Vec<IVec2>` (both ends included, each step a neighbour), `range(center, radius: u32) -> Vec<IVec2>` (`center` first, then each ring outward), `ring(center, radius) -> Vec<IVec2>` (the cells exactly `radius` steps away, clockwise), `bounds_of(CellRect) -> Rect`, `lattice() -> Option<Lattice>` (the square lattice its cells make; `None` on a hexagonal grid). `GridShape::{Orthogonal (default), Isometric, Staggered { axis: StaggerAxis, index: StaggerIndex }, Hexagonal { axis, index, side: u32 (Tiled's hexsidelength; 0 makes the staggered grid's diamonds, keeping six neighbours) }, Oblique { skew: IVec2 (each row shifts x by `skew.x` pixels, each column y by `skew.y`) }}` (`Copy`, `Eq`, `Hash`); `StaggerAxis::{X, Y}` (the columns or the rows shifted half a cell: hexagons flat-topped for X, pointy-topped for Y); `StaggerIndex::{Odd, Even}` (which of them). `CellOutline`: `points() -> &[Vec2]` (4, or 6 on hex maps, where a side of length 0 repeats its point). `Lattice { to_pixel: Affine2 }` (`Copy`, `PartialEq`): lattice cell (x, y) is the unit square at (x, y), `to_pixel` takes it to the layer's pixels, so lattice cells side by side are neighbours on the grid; `lattice_cell(IVec2) -> IVec2` (a grid cell's: the same cell but on staggered grids, which index their diamonds' isometric lattice otherwise) and `grid_cell(IVec2) -> IVec2` (undone): a square generator's cells placed at `grid_cell` keep their neighbours on orthogonal, isometric, staggered and oblique maps. A map's is `TileMap::grid()`, a spawned layer's `TileLayerData::grid`.
- `Tile { tileset: u16, id: u32, flip: TileFlip }`: `new(tileset, id)`, `with_flip(TileFlip)`. `TileFlip::{NONE, HORIZONTAL, VERTICAL, DIAGONAL, HEX_ROTATE_60, HEX_ROTATE_120}` (Tiled's bits: diagonal first, then horizontal, then vertical; on a hexagonal map the diagonal bit is `HEX_ROTATE_60`, a 60° turn clockwise, and `HEX_ROTATE_120` a 120° one, both after the flips): `contains`, `|`, `rotated_cw()` (90°, square grids), `rotated_hex_cw()` (60°, a half turn written as both flips; six give back the start).
- `TileData { class, properties, shapes: Vec<TileShape>, animation: Vec<TileFrame>, probability: f32, sort_offset: f32, stairs: Option<u8>, image: Option<(Handle<Image>, UVec2)> }` (`Default`, probability 1, `sort_offset` 0: the reserved tile property that moves the tile's sort point down by that many pixels on a y-sorted layer; `stairs` `None`: the reserved int tile property naming the floor a stair or ramp tile leads up to from its layer's floor, a whole number from 0 to 255, else the map is refused, naming the tileset and tile). `TileShape { points: Vec<Vec2> (tile pixels from the top-left), one_way: bool, occluder: bool (blocks light only), properties }` (`Default`). `TileFrame { tile: u32, duration: Duration }`.
- `WangSet` (read-only once built): `new(name, WangKind, Vec<WangColor>, Vec<WangTile>, TileTransformations)`, `name()`, `kind()`, `colors()`, `tiles()`, `transformations()`, `color_index(&str) -> Option<u8>` (1-based), `best_tile(wanted: [u8; 8], cell: IVec2) -> Option<(u32, TileFlip)>` (positions clockwise from the top edge; 0 no colour, `WangSet::ANY` any; ties broken by a hash of `cell`), `ANY: u8`. `enum WangKind { Corner, Edge, Mixed }`; `WangColor { name, color: Color, tile: Option<u32>, probability, properties }`; `WangTile { tile: u32, wangid: [u8; 8], probability: f32 }`; `TileTransformations { hflip, vflip, rotate, prefer_untransformed: bool }`.
- `Properties` (in written order): `get(&str) -> Option<&PropertyValue>`, `bool`, `int -> Option<i64>` (a whole float counts), `float -> Option<f64>` (an int counts), `string -> Option<&str>`, `color -> Option<Color>`, `insert(name, PropertyValue)`, `iter()`, `len()`, `is_empty()`. `enum PropertyValue { Bool(bool), Int(i64), Float(f64), String(String), Color(Color), File(String), Object(String) (an object's id), Enum { ty, value }, Class { ty, fields: Properties }, List(Vec<PropertyValue>) }`.
- `URect { min: UVec2, max: UVec2 }` (texels, `max` exclusive): `new(x0, y0, x1, y1)`, `size()`.

**Spawned maps:**

| Type | API |
|---|---|
| `MapRoot` (component, brings `Transform`, which places the map's cell (0, 0): its top-left, or an infinite map's tile origin) | `new(Handle<TileMap>)`, `ldtk(Handle<LdtkProject>, level: &str)` (identifier or iid), `world_level(&Handle<TileWorld>, level: usize)`, `ldtk_world_level(Handle<LdtkProject>, world: &str, level: usize)`; `with_entity_tiles(&[&str])` (tiles of these classes spawn as `MapTile`s, neither drawn nor collided), `with_intgrid_collision(impl Into<IntGridKey>, TileCell)` (`TileCell::Shape` collides as nothing, warned once), `without_collision()`, `with_z(i32)` (the bottom layer's z; layer `i` at `z + i`). It lands whole once its assets load: layers, objects, class tiles, colliders, shadows and `MapEvent::Spawned` in one frame. Despawning it despawns the map |
| `enum IntGridKey { Value(i32), Name(String) }` | `From<i32>`, `From<&str>`; by default the names `solid`, `one_way`, `slope_bottom_left`, `slope_bottom_right`, `slope_top_left`, `slope_top_right` (any case) collide as they say |
| `MapLayerRef` (component on each layer entity) `{ map: Entity, index: usize (bottom first), name: String }` | with `LayerInfo`, a `Transform` at the layer's offset (less its floor's height on a floor above the ground), and `TileLayerData` or `ImageLayer` |
| `TileLayerData` (component) `{ tiles: TileGrid, intgrid: Option<IntGrid>, grid: MapGrid }` | the spawned layer's own cells; editing `tiles` redraws and re-collides as `ctx.maps()` does; `grid` places its cells in the layer's pixels |
| `MapObject` (component, child of its object layer, `Transform` at its rectangle's centre; with `Floor::On(n)` on a layer on storey `n` from 1) `{ map: Entity, id: String, name, class: Option<String>, shape: ObjectShape, size: Vec2, tile: Option<Tile>, visible: bool, properties }` | `reference(field: &str, world: &World) -> Option<Entity>` (an object-reference property's object, if alive). An `On<Add<MapObject>>` observer sees each as it arrives, the map's colliders and every object's entity already in |
| `MapTile` (component, child of its tile layer at the cell's centre; with `Floor::On(n)` on a layer on storey `n` from 1) `{ map: Entity, layer: usize, cell: IVec2, tile: Tile, class: String, properties }` | a class tile |
| `MapIds` (component on the map entity) | `get(id: &str) -> Option<Entity>` |
| `MapFloors` (component on the map entity when some layer is above the ground floor; `Clone`, `PartialEq`) | `lift(Floor) -> Option<Vec2>` (the floor's height in map pixels, what it is drawn lifted by: the vertical offset of its bottom layer; `GROUND` gives zero; a floor no layer is on, or `Every`, `None`), `floors() -> impl Iterator<Item = Floor>` (ascending, the ground floor first), `highest() -> Floor`. A reload replaces it (or takes it away) |
| `Climber` (component, `Copy`, `Default`, `Eq`; brings `Floor`) | opts an entity into stairs (tiles whose `stairs` names the floor they lead up to, on a lower floor's layer): each fixed step, before physics, a climber on a run of stairs on either of its two floors goes onto the upper floor halfway up it or more and onto the lower below that, by where the last step left it, sending `FloorChanged` (a step in which something else wrote its `Floor`, a teleport or a respawn, leaves that floor be); all along the run it draws lifted between the two floors' heights by how far up it is (where it draws, interpolated between steps), with everything below it in the hierarchy. Without it an entity keeps its floor on stairs. A run meets its upper floor only at its top |
| `FloorChanged` (message, `Copy`, `Eq`) `{ entity: Entity, from: Floor, to: Floor }` | a `Climber` changed floor on stairs, once per change; that step's physics already has it on `to`, and `update` reads it the step after |
| `MapFloor` (component on a child of the map entity, one per floor above the ground with a tile layer; `Clone`, `Copy`, `PartialEq`, `Eq`) `{ floor: u8 }` | holds that floor's edge: a `TileColliders` (floor `On(floor)`, the map's material) of its footprint, the cells its tile layers fill and the stairs up to it on lower floors' layers, where its bottom tile layer stands (its `Transform`); the hollow outline keeps what is on the floor on it. Kept in step with the tiles (an edit keeps the entity), taken away with the map's colliders, and replaced by every full rebuild of them (the map enabled again, a change of its layers, their places, floors or materials, a tileset saved with other shapes or stairs), so find it again rather than keep it; blocks no navigation, and is no surface to particles |
| `MapOccluder` (component) `{ chunk: IVec2 }` (signed, as `TileGrid::chunk_of` gives it; on a staggered map a chunk of its lattice's cells) | marks the `LightOccluder2D::chain`s a map keeps from its tiles (children of its first occluding layer on each storey; with `Floor::On(n)` on storey `n` from 1, so they block only that storey's lights and those on `Floor::Every`); an edit rebuilds its chunk's chains (on a hexagonal map only the chains through the cells it changed) |
| `WorldLevelOf` (component) `{ world: WorldRef, level: usize }` | marks a root `spawn_levels_in` spawned; despawn the root to unload the level (a disabled root keeps it) |
| `enum WorldRef { Tiled(Handle<TileWorld>), Ldtk { project: Handle<LdtkProject>, world: String } }` | `From<&Handle<TileWorld>>`; a single-world LDtk project's world is "World" |
| `enum MapEvent { Spawned { map }, Reloaded { map }, Failed { map, error: Arc<AssetError> } }` (message) | landed whole this frame; followed a saved file; did not load or land (sent once per failure) |
| `enum LayerRef<'a> { Name(&'a str), Index(usize) }` | `From<&str>`, `From<&String>`, `From<usize>`; a name is the first layer so called |
| `MapStream` (component, `Clone`, on a `MapRoot` entity) `{ generator: Arc<dyn Fn(ChunkRequest) -> ChunkTiles + Send + Sync>, load_distance: f32, unload_distance: f32, cameras: bool, land_per_frame: u32, unload_per_frame: u32, web_budget: Duration, saved: Option<SavedChunks> }` | `new(impl Fn(ChunkRequest) -> ChunkTiles + Send + Sync + 'static)` (load 512 px, unload 1,536 px, cameras on, 2 land and 2 unload a frame, 2 ms, no saved chunks), `with_saved(SavedChunks) -> MapStream`. Streams an infinite map's unbounded tile layers, on any grid (by the first one's grid; one on another grid is warned once): chunks within `load_distance` (world px, along each axis) of a camera's view (while `cameras`) or a `StreamAnchor` are asked for (on an orthogonal map those the area overlaps, on another those whose box meets it) on the `ryo2d chunk builder` thread, nearest first (from `saved` if it holds them, else the generator), and land at most `land_per_frame` a frame, nearest first, in every tile layer (an empty chunk where the generator gives a layer nothing); chunks past `unload_distance` from every one unload, at most `unload_per_frame` a frame, the farthest first, none in a frame that lands while eight or fewer wait, the others resident until their turn (an `unload_distance` below `load_distance` is raised to it, warned), each changed since it landed going into `saved`, and their `ChunkOwned` entities despawn. An anchor's load area is capped at 64 chunks a side (warned). On native platforms a generator that panics leaves its chunk empty, never asked again, warned once a map; a web build aborts on the panic. On the web the generator runs on the main thread, `web_budget` a frame (at least one chunk). Only resident chunks take edits. On a Tiled infinite map every engine chunk any layer's Tiled chunk touches is the file's (as is every chunk a map built in code holds in its asset): never generated, resident from the start, never unloaded, no `ChunkEvent`; their edits are kept through `save_changed_chunks` and as the root goes, and put back from `saved` as a stream starts. A hot reload changes the file's chunks alone. When the root goes (despawned, a scene's cleanup, or a `MapRoot` put over it) its changed resident chunks go into `saved`, with no `ChunkEvent` |
| `StreamAnchor` (component, `Default`) | keeps the chunks near its entity loaded on every streamed map |
| `ChunkTiles` (`Clone`, `Default`, `PartialEq`: the same tiles; `Serialize`, `Deserialize`: each layer holding a tile to its cells as base64 zlib) | a chunk's tiles by layer name: `set(layer: &str, cell: UVec2, Option<Tile>)` (`cell` within the chunk, 0..32 each way; outside it, or a tile that does not fit a cell, changes nothing), `get(layer, cell) -> Option<Tile>`, `is_empty() -> bool` |
| `SavedChunks` (`Clone`: the same store, `Default`, `Debug`, `Serialize`, `Deserialize`) | the chunks a player changed (a tile edit, a class tile an edit placed or the game despawned), with their class tiles, kept as they unload or as the map's root goes and brought back instead of the generator's: `get(chunk: IVec2) -> Option<ChunkTiles>`, `insert(chunk, ChunkTiles)`, `remove(chunk) -> Option<ChunkTiles>` (it regenerates), `contains(chunk) -> bool`, `keys() -> Vec<IVec2>` (the chunks kept, by row then column: a copy), `len() -> usize`, `is_empty() -> bool`. Give it to a stream with `MapStream::with_saved`; serialize it with a save game (a broken one is a deserialize error), after `MapsView::save_changed_chunks` |
| `StreamStatus { resident: usize, generated: usize, hand_made: usize, in_flight: usize, draining: usize }` (`Copy`, `Default`, `PartialEq`) | what a streamed map has, from `MapsView::stream_status`: the chunks resident; of them those the stream landed and the file's (a Tiled infinite map's own, or those a map built in code holds in its asset); the chunks asked for and not landed (out on the chunk builder or waiting to land, 64 held at most); the resident chunks past every unload distance waiting their turn under `unload_per_frame` |
| `ChunkOwned` (component, `Copy`) `{ map: Entity, chunk: IVec2 }` | despawned (with its children, disabled or not) when the chunk unloads or the map's root goes: for the enemies and pickups a game places on `ChunkEvent::Loaded` |
| `ChunkRequest { chunk: IVec2, first_cell: IVec2 (chunk * 32), grid: MapGrid (the first unbounded tile layer's) }` (`Copy`, `PartialEq`) | what a generator is asked for; a `ChunkTiles` cell `c` is the map's cell `first_cell + c` |
| `enum ChunkEvent { Loaded { map, chunk: IVec2 }, Unloading { map, chunk, changed: Option<ChunkTiles> } }` (message) | a streamed chunk landed this frame (its class tiles in); one unloaded this frame: by the time it is read the chunk's tiles, class tiles and owned entities are gone (its colliders and shadows within the frame), and `changed` holds its tiles, class tiles included, when the player changed it since it landed (`None` otherwise) |

`MapsView` (`ctx.maps()`, or `MapsView::new(&mut World)`): every call names the map entity and, where it reads cells, a layer (`impl Into<LayerRef>`); a map not landed, a missing or non-tile layer, or a cell outside a bounded layer reads `None` / `false` / 0. An unbounded layer (`TileGrid::unbounded`) takes any cell: `set_tile`, `fill` and `set_terrain` make the chunks they write in, which draw, collide (within 186,411 cells of (0, 0)) and cast shadows. On a map with a `MapStream`, only its resident chunks take edits: `set_tile`, `set_terrain` (the painted cell) and `set_intgrid` elsewhere return false, and `fill` and `fill_intgrid` change only the cells in resident chunks.

| Method | |
|---|---|
| `is_ready(map) -> bool` | landed |
| `layer_entity(map, layer) -> Option<Entity>` | |
| `bounds(map) -> Option<CellRect>` | the cells the map covers as it is now: a bounded map's (0, 0) to its size, an infinite map's smallest rectangle holding every cell of its tile layers with a tile (class tiles included), following edits; `None` until it lands |
| `layer_bounds(map, layer) -> Option<CellRect>` | a tile layer's: a bounded layer's bounds, an unbounded one's cells holding tiles as they are now; `None` for a layer the map lacks or not a tile layer |
| `stream_ready(map, area: Rect) -> bool` | every chunk `area` (world px) takes is resident on the streamed map (landed, or one of the map's own: a Tiled infinite map's file chunks, or those a map built in code holds in its asset), as a view's area takes them: on an orthogonal map those it overlaps (an area of no width or height asks for the chunks holding it), on any other those whose box meets it: for a loading screen after a teleport; false for a map not streamed or not landed |
| `stream_status(map) -> Option<StreamStatus>` | the streamed map's counts as this frame's stream left it (it runs in `PreUpdate`); `None` for a map not streamed or not landed |
| `save_changed_chunks(map) -> usize` | puts each resident chunk of the streamed map changed since it landed into its stream's `SavedChunks`, as unloading would (a Tiled infinite map's own chunks, which never unload, only so): call it before writing them into a save game; how many it put (0 with no stream or no `SavedChunks`) |
| `tile(map, layer, cell: IVec2) -> Option<Tile>` | a class tile's cell reads `None` |
| `set_tile(map, layer, cell, Option<Tile>) -> bool`, `fill(map, layer, min, max, Option<Tile>) -> usize` | edits; a registered class's tile spawns a `MapTile` instead; each redraws, re-collides and re-shadows only the chunks it touches (a layer with an IntGrid collides by its values, not its tiles); a tile `fill` that would make more than 131,072 chunks of an unbounded layer is refused (0, warned once a layer) |
| `set_terrain(map, layer, cell, wang_set: &str, color: Option<&str>) -> bool` | Wang painting as Tiled's terrain brush with corrections on; `None` erases; false within a few cells of the ends of `i32`, where the cells the paint reaches would not fit, and on a hexagonal map (warned once); a staggered map paints in its isometric lattice, as Tiled does |
| `intgrid(map, layer, cell) -> Option<i32>` | |
| `set_intgrid(map, layer, cell, value: i32) -> bool`, `fill_intgrid(map, layer, min, max, value: i32) -> usize` | IntGrid edits (0 empties; false / 0 for a layer with no IntGrid); only the chunks changed collide and cast shadows again. LDtk's auto-layer rules do not run: the tiles stay as they were |
| `tile_data(map, Tile) -> Option<TileData>` | |
| `world_to_cell(map, layer, Vec2) -> Option<IVec2>`, `cell_to_world(map, layer, IVec2) -> Option<Vec2>` | through the map's and layer's `Transform`s as they are now and the layer's `MapGrid`: the cell whose outline holds the point; a cell's centre |
| `grid(map, layer) -> Option<LayerGrid>` | a tile layer's grid and its place in the world as they are now, for picking, outlines and the board tools on every grid, in ground space (a layer on a floor above the ground draws its floor's height higher); `None` for a map not landed, a layer it lacks or not a tile layer |
| `floors(map) -> Option<&MapFloors>` | the map's floors and their heights in its pixels; `None` for a map not landed or with no layer above the ground floor |
| `lift(map, Floor) -> Option<Vec2>` | the floor's height as a world vector, through the map's transform as it is now (moved, turned or scaled); `GROUND` gives zero on any landed map; a floor no layer is on, `Every`, or a map not landed gives `None` |
| `floor_cell_at_world(map, world: Vec2) -> Option<(Floor, IVec2)>` | the floor and the ground cell drawn under a world point: floors from the highest down, each through its bottom tile layer's grid lifted by its height; a floor above the ground answers only where one of its tile layers on that grid that draws (visible, not disabled) holds a tile or class tile, the ground floor anywhere; the cell is the bottom layer's, in ground space; through the map's transform as it is now; `None` for a map not landed, or with no tile layer on the ground floor where nothing above answers |
| `drawn_lift(entity) -> Vec2` | how far up an entity draws, a world vector: on a `Floor` (or below an entity with one), its storey's height on the map with floors under where it stands (the first in entity order where maps overlap), turned with that map; zero on the ground floor, off every map with floors, or without a `Floor` above it; worked out after transforms propagate, each fixed step and each frame. A camera following a character adds it to the character's `GlobalTransform` |
| `y_sort_z(map) -> Option<i32>` | the z of the map's first y-sorted tile or object layer (the map's z plus its index; an image layer does not count), which on a map with floors every y-sorted layer draws at: the z for characters to sort with them; `None` with no y-sorted layer or a map not landed |
| `navigation(map, layer, window: CellRect) -> Option<NavigationGrid>` | (`&mut self`) `NavigationGrid::for_layer` over `window` (max exclusive) of the layer's cells, on the layer's storey (`on_floor`), with every `TileColliders` of the map on that storey (the map entity's and its layers', whose `floor` meets it; a storey's edge is none) blocked through `block_tiles`, each placed by its entity's whole transform as it is now, and on a storey above the ground every cell outside its footprint (its tile layers' cells and the stairs up to it) blocked; it runs the maps' collision sync first, so it reads tiles edited or landed earlier in the same `update` (under a microsecond a map when nothing changed); on a map with a `MapStream`, every cell of an unbounded layer in a chunk not resident is `Blocked`; a copy: add the game's colliders and costs, and take it again after `ChunkEvent`s or edits (a new grid, its `generation` counted afresh: find old paths again); `None` for a map not landed, a layer it lacks or not a tile layer |
| `floor_navigation(map, window: CellRect) -> Option<FloorNavigation>` | (`&mut self`) each storey's `navigation` of its bottom tile layer over `window` of the ground floor's cells, joined by a `StairLink` at each cell of the map's runs of stairs (from the storey of the stairs' layer to the storey they lead up to); a storey whose bottom tile layer stands elsewhere than the ground floor's is left out (warned once a map); a map without storeys gives its ground floor's grid alone; it syncs the map's collision and stairs first; a copy: for the game's own obstacles, clone its `grids()`, change them and make it again with `FloorNavigation::new(grids, nav.links().to_vec())`; `None` for a map not landed or with no tile layer on the ground floor |
| `spawn_levels_in(world: impl Into<WorldRef>, area: Rect) -> Vec<Entity>` | spawns each level at world depth 0 overlapping `area` not already spawned (a `MapRoot` at its `origin` with a `WorldLevelOf`; a disabled root counts as spawned); returns those it spawned |
| `spawn_levels_in_with(world, area, depth: i32, configure: impl FnMut(&WorldLevel, MapRoot) -> MapRoot) -> Vec<Entity>` | the same at world depth `depth`, each root set up by `configure` (its z, class tiles, IntGrid collision, collision); the map stays the level's |

- `LayerGrid { grid: MapGrid, to_world: Affine2 }` (`Copy`, `PartialEq`, from `MapsView::grid`): `cell_at_world(Vec2) -> IVec2` (the cell whose outline holds the world point; (0, 0) for a layer scaled to nothing, where `world_to_cell` is `None`), `cell_center_world(IVec2) -> Vec2`, `outline_world(IVec2) -> CellOutline` (in the world, clockwise on screen from its top-most point, on a turned or mirrored map too); the board tools are `grid`'s (`neighbours`, `distance`, `line`, `range`, `ring`). It holds the transforms as read: read it again after the map moves or reloads. It is in ground space: on a floor above the ground its cells are where that floor's characters stand, the floor's height below where they draw (`MapsView::floor_cell_at_world` picks what is drawn).
- Collision: a map's colliding layers keep `TileColliders` (see [Characters and gameplay](#characters-and-gameplay)) on the map entity, or on a layer with its own grid, offset, floor or material (`collision_layer`, `collision_mask`, `friction`, `restitution` properties, a layer's over the map's), each grid on its layers' floor (`TileColliders::floor`); each floor above the ground has an edge on a `MapFloor` child of the map. Parallax is ignored for collision. Isometric, staggered and oblique maps collide and cast shadows along their cells' outlines, their grids holding their lattice's cells (`TileColliders::lattice`; a staggered cell `c` at `lattice.lattice_cell(c)`), and their one-way tiles collide both ways (warned once a map). Hexagonal maps collide along their hexagons' sides: a tile whose shape is its cell's whole hexagon is a solid cell however turned, and any other shape collides as its tile's own closed rings, flipped and turned as the tile draws, not merged with its neighbours; a hexagonal layer casts its shadows along the same sides and shapes, and hexagons over 16,384 px or under 2 px neither collide nor cast shadows (warned once a layer).
- Tiled maps of every grid load with it (`TileMap::shape`: orientation, stagger axis and index, hex side length, skews), a hexagonal map's tiles keeping their 60° and 120° turns (`TileFlip::HEX_ROTATE_60`, `HEX_ROTATE_120`), which load off hexagonal maps without the 120° bit, as in Tiled. Refused, naming what: an orientation, stagger axis or index Tiled has none of, a negative hex side length, and layer blend modes other than normal, add, multiply and screen.

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

```rust,ignore
// Signatures
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

## Config, settings and saves

Three homes: the project's `Ryo.toml` (`Workspace`), the player's settings (`Settings`, kept in `system.json`) and the game's saves (`ctx.saves()`, one `<slot>.ryo-save` a slot). The engine writes `system.json` and the save files; the game only sets values and calls `save`. See `ryo-config-save` for the guide and the limits.

| `SavesView<'w>` (`ctx.saves()`; `SavesView::new(&World)`) and `Saves<'w>` (a `SystemParam`: `saves: Saves`) | |
|---|---|
| `save<T: Serialize + Send + 'static>(&self, slot: &str, data: T) -> Result<(), SaveError>` | checks the slot and queues the save, which is serialized, compressed and written crash-safely on the save thread (on the frame, on the web), in call order; fails at once only for `InvalidSlot` or `Unavailable`; a `SaveFinished` reports the write |
| `save_with<T: Serialize + Send + 'static>(&self, slot: &str, data: T, info: SaveInfo) -> Result<(), SaveError>` | with a display name, play time and labels for the slot list |
| `load<T: DeserializeOwned + 'static>(&self, slot: &str) -> Result<Loaded<T>, SaveError>` | at once; a queued save loads as queued; upgraded through the `save_versions` steps; a damaged save loads its backup (`from_backup`, warned), both damaged is `Corrupt`; a newer build's is `NewerVersion`; one that does not read as `T` is `Deserialize` |
| `info(&self, slot: &str) -> Result<SaveHeader, SaveError>` | the header, without the data |
| `list(&self) -> Result<Vec<SaveSlot>, SaveError>` | every slot with its header, sorted by slot, queued ones included; reads headers only; a damaged or newer slot is left out |
| `exists(&self, slot: &str) -> bool`, `is_writing(&self, slot: &str) -> bool` | a save is kept or queued; from a save or delete until its `SaveFinished` |
| `delete(&self, slot: &str) -> Result<(), SaveError>` | queues the removal of the save and its backup, after the slot's earlier saves |

- Slot names: 1 to 64 of `a`-`z`, `0`-`9`, `-` and `_`, not a Windows device name (`con`, `prn`, `aux`, `nul`, `com1`-`com9`, `lpt1`-`lpt9`). An older engine's save under another name still loads, lists and deletes on native.
- `SaveInfo { name: Option<String>, play_time: u64, labels: BTreeMap<String, String> }` (`Default`): `new()`, `name(impl Into<String>)`, `play_time(secs: u64)`, `label(key: impl Into<String>, value: impl Into<String>)`.
- `Loaded<T> { data: T, header: SaveHeader, from_backup: bool, upgraded_from: Option<u32> }`; `SaveSlot { slot: String, header: SaveHeader, from_backup: bool }`.
- `SaveHeader { format: u32, version: u32, saved_at: u64, play_time: u64, name: Option<String>, labels: BTreeMap<String, String>, engine_version: String }`: the file format (2, or 1 for an older engine's save), the data version, the save time in Unix seconds, and an empty `engine_version` in format 1.
- `SaveFinished { slot, op: SaveOp, result: Result<(), Arc<SaveError>> }` (message), `enum SaveOp { Save, Delete }`: see [Messages](#messages).
- `SaveVersions<T>`: `new(current: u32)`, `step<A: DeserializeOwned + 'static, B: Serialize + 'static>(self, from: u32, convert: impl Fn(A) -> B + Send + Sync + 'static) -> Self` (version `from`'s value into `from + 1`'s; the last step yields `T`). Registered with `EngineBuilder::save_versions` or `game_settings_versioned`; a type never registered is version 1. A missing, duplicate or out-of-range step, or version 0, panics at the registering call.
- On quit the engine finishes queued saves (warning after 3 s, giving up after 10 s).

| `Settings` (resource; `ctx.settings() -> Mut<'_, Settings>`, `Res<Settings>` / `ResMut<Settings>` in systems) | |
|---|---|
| `volume(&self, kind: VolumeKind) -> f32`, `set_volume(&mut self, kind: VolumeKind, value: f32)` | 0 to 1 on the curve `volume_to_db`; clamped, NaN is the game's default |
| `window_size(&self) -> (u32, u32)`, `set_window_size(&mut self, width: u32, height: u32)` | the window's size in logical pixels while windowed, each side at least 1 |
| `display_mode(&self) -> DisplayMode`, `set_display_mode(&mut self, mode: DisplayMode)` | |
| `vsync(&self) -> bool`, `set_vsync(&mut self, vsync: bool)` | reconfigures the surface (nothing on the web) |
| `show_fps(&self) -> bool`, `set_show_fps(&mut self, show: bool)` | the FPS counter |
| `shadow_quality(&self) -> ShadowQuality`, `set_shadow_quality(&mut self, quality: ShadowQuality)` | how finely lights' shadows are drawn (default `High`, or the game's: `[system] shadow_quality`, `EngineBuilder::shadow_quality`) |
| `reset(&mut self, preference: UserPreference)`, `reset_all(&mut self)` | back to the game's defaults (`reset_all` also puts each section to its `Default`); bindings reset with `ctx.actions().reset_defaults()` |
| `is_kept(&self, preference: UserPreference) -> bool` | whether `system.json` keeps it (`with_user_preferences`) |
| `game<T: 'static>(&self) -> &T`, `game_mut<T: 'static>(&mut self) -> &mut T` | the game's section of type `T`; panics for a type not registered with `game_settings` |

- A change applies the same frame, after the fixed steps (the window's after the frame), and is written a second after the last change to what the file keeps, and at once when the game quits or is hidden. The player's resizes and fullscreen toggles (the OS, F5) update `Settings` too.
- `system.json` (format 3) keeps the kept settings that differ from the game's defaults, the bindings that differ from theirs, and the sections' changed fields; formats 1 and 2 convert as they load. `SettingsSaveFailed { error: Arc<SaveError> }` (message) reports a failed write.
- `enum UserPreference { MasterVolume, MusicVolume, SfxVolume, UiVolume, VoiceVolume, AmbienceVolume, Resolution, DisplayMode, Vsync, ShowFps, ShadowQuality, Bindings }` (`all()`; `Resolution` is the player's window size); `UserPreferenceFilter`: `all()` (also `Default`), `none()`, `only(iter)`, `contains`, `is_empty`, `len`, and `From` an array, `Vec` or slice of `UserPreference`.
- `enum DisplayMode { Windowed, ExclusiveFullscreen, BorderlessFullscreen }` (`all()`, `display_name()`); `Resolution { width, height }` (`new`, `common()`, `display_name()`); `parse_resolution("1280x720") -> Option<(u32, u32)>`.
- `Workspace` (`Ryo.toml` project; native only, `discover()` fails on the web): `discover()`, `discover_from(dir)`, `from_path(root)`, `root()`, `name()`, `config() -> &WorkspaceConfig`, `entry_point()`, `assets_dir()`, `save_location()`, `save_data_dir()`, `set_save_data_path(p)`, `has_config_file()`.
- `WorkspaceConfig { name, title, description, version, entry_point, save_data, assets, debug: DebugConfig, release: ReleaseConfig, tiled_project: Option<String>, env }`: `env_str/env_bool/env_int/env_float(key)`, `has_env(key)`; `ReleaseConfig { logo, portable, save_folder }` (`ryo2d::core::config`).
- `ryo2d::project!()`, or `project!("path/from/the/crate.toml")`: the crate's `Ryo.toml` compiled in as a `ryo2d::core::config::ProjectFile` (`from_toml(&'static str)`, `none()`, `is_none()`, `text()`), for `EngineBuilder::project`.

<!-- with: #[derive(serde::Serialize, serde::Deserialize)] struct Progress { level: u32 } -->
```rust
ctx.saves().save("slot1", Progress { level: 3 })?;
let loaded: Loaded<Progress> = ctx.saves().load("slot1")?;
ctx.settings().set_volume(VolumeKind::Music, 0.5);
```

See `ryo-config-save`.

---

## Text

Fonts, styles, rich text, effects and world text. The guide is `ryo-text`.

| `Fonts` (resource; `ctx.fonts()`) | |
|---|---|
| `face_of(impl Into<AssetId<Font>>) -> Option<FontId>` | the face a font asset registered as: `None` while it is not loaded, and once it is freed. A font registers while it is in its store (`ctx.assets().add(Font::from_bytes(bytes)?)` or `load_now::<Font>(path)`) and leaves, with its glyphs, once freed; two fonts of the same bytes and face share one face |
| `find(&str) -> Option<FontId>` | the family's regular face (weight 400, upright) |
| `find_face(family: &str, weight: f32, italic: bool) -> Option<FontId>` | the face nearest `weight` preferring the slant; on ties the face added first wins |
| `family(FontId) -> Option<&str>`, `role(FontRole) -> Option<FontId>`, `role_font(FontRole) -> Option<&Handle<Font>>`, `set_role(FontRole, Handle<Font>)` | families and role fonts: the role holds its font; one still loading takes over once it registers, the role keeping its old face meanwhile (setting the font a role already uses changes nothing) |
| `set_render(FontId, FontRender)`, `render(FontId) -> FontRender` | how a font draws (the mode it already has changes nothing) |
| `prewarm(FontId, size: f32, chars: &str)`, `generation() -> u64` | build glyphs ahead (MSDF glyphs cost about 0.46 ms each); `generation` changes on every real font change |

- `Fonts` has no public constructor: the engine owns the one registry (the glyph caches key on its ids), and it follows the font stores: a font added in a scene is there in the same `ctx.fonts()`. Without the `default-fonts` feature it starts empty.
- `Font` (an asset; `.ttf`, `.otf`, `.ttc` files, `FontSettings { face }` for a collection's face): `from_bytes(impl Into<Arc<[u8]>>) -> Result<Font, FontError>`, `from_bytes_face(bytes, index: u32)`, `family() -> &str`. Embed with `Font::from_bytes(include_bytes!(..).as_slice())` (the web has no files). `BitmapFont { family, sheet: Handle<Image>, cell: (u32, u32), chars }` (an asset; `.bitmap.ron` files, whose sheet is a dependency): `new(family, sheet, cell, chars)`; text reaches it by its family.
- `FontId` (`Copy`, `Eq`, `Hash`, `Ord`). `enum FontRole { Ui (default), Mono, Pixel }`: Inter, JetBrains Mono, m5x7 (feature `default-fonts`, on by default).
- `enum FontRender { Raster (default), Pixel { design_size: f32 }, Msdf { base: f32, range: f32 } }` (`#[non_exhaustive]`); `FontRender::pixel(design_size)`; `FontRender::msdf()` is base 32, range 4. Pixel sizes snap to whole multiples of the design size. MSDF effect reach is `range / 2` base pixels; MSDF fonts get no synthesized italic or bold.
- `enum FontError { Invalid, NoFace(u32), EmptyBitmap }`.

| `TextStyle` (`#[non_exhaustive]`) | |
|---|---|
| fields | `font: TextFont` (default `Role(Ui)`), `fallbacks: Vec<TextFont>`, `size: f32` (14.0), `weight: f32` (400.0), `italic: bool`, `color: Color` (white), `line_height: f32` (1.2), `letter_spacing: f32` (0.0), `outline: Option<Outline>`, `shadow: Option<Shadow>`, `glow: Option<Glow>` |
| builders | `new()`, `font(TextFont)`, `role(FontRole)`, `family(impl Into<String>)`, `size(f32)`, `weight(f32)`, `bold()`, `italic()`, `color(Color)`, `line_height(f32)`, `letter_spacing(f32)`, `outline(width, Color)`, `shadow(offset: Vec2, Color, softness)`, `glow(radius, Color)`, `fallback(TextFont)` |

- `enum TextFont { Role(FontRole), Family(String), Face(Handle<Font>) }`: `Face` is one exact font, held by the style, at its own weight and slant (one still loading falls back as an unknown family does). `Outline { width, color }`, `Shadow { offset: Vec2, color, softness }`, `Glow { radius, color }`. Glow needs an MSDF font; shadow softness only applies to MSDF fonts.
- `TextLayoutOptions` (`#[non_exhaustive]`, `Default`): fields `wrap: Option<f32>`, `align: TextAlign`, `max_lines: Option<u32>`, `overflow: TextOverflow`, `anchor: Anchor` (default `TopLeft`); builders `wrap(f32)`, `align(TextAlign)`, `max_lines(u32)`, `overflow(TextOverflow)`, `anchor(Anchor)`. `enum TextAlign { Start, Center, End, Justify }`, `enum TextOverflow { Clip, Ellipsis }`.

| Rich text | |
|---|---|
| `RichText` | `new(impl Into<String>)`, `markup(&str)`, `escape(&str) -> Cow<str>` (doubles every `[`, so untrusted text can go inside markup), `plain(&str)`, `span(&str, Span)`, `icon(Handle<Image>)`, `icon_sized(Handle<Image>, Vec2)`, `icon_named(impl Into<String>)`, `pause(seconds)`, `prompt(action: &str)` (the key or button that triggers `action` on the last device, as a registered glyph or a label such as `[A]` in the accent colour), `as_str()`, `is_empty()`; `From<&str>` and `From<String>` (plain, no markup) |
| `Span` (`#[non_exhaustive]`, `Default`) | fields `color`, `size`, `font`, `weight`, `italic`, `outline`, `effect` (all `Option`); constructors `new()`, `color(Color)`, `size(f32)`, `font(TextFont)`, `bold()`, `italic()`, `outline(width, Color)`, `effect(impl Into<String>)`; builders `with_color`, `with_size`, `with_font`, `with_weight`, `with_italic`, `with_outline`, `with_effect` |
| `Icon` | `enum Icon { Sprite { sprite: Handle<Image>, size: Option<Vec2> }, Named { name: String, size: Option<Vec2> } }` |
| `TextIcons` (resource) | `insert(name, Handle<Image>)`, `insert_region(name, Handle<Image>, Rect)`, `get(&str) -> Option<&IconSource>`, `remove(&str)` (re-insert a name to swap an icon set) |
| `IconSource` | `sprite: Handle<Image>`, `region: Option<Rect>` (pixels on the image; `None` is the whole image) |
| `TextEffects` (resource) | `register(name, Fn(&GlyphContext) -> GlyphMod + Send + Sync + 'static)` (a pure function of the context: it runs per glyph per pass per camera), `contains(&str)`; built in `wave`, `shake`, `rainbow` |
| `GlyphContext` (`#[non_exhaustive]`) | `index: u32`, `cluster: u32`, `line: u32`, `time: f32` (real time in the UI, game time in the world), `size: f32`, `seed: u64`, `args: String` |
| `GlyphMod` | `offset: Vec2`, `scale: f32`, `color: Option<Color>`, `alpha: f32`; `Default` is no change |

- Markup tags: `[color=..]`, `[size=..]`, `[font=..]`, `[b]`, `[i]`, `[outline=width,color]`, `[icon=name]`, `[pause=seconds]`, `[prompt=action]` (a glyph `TextIcons` has as `prompt.key.Space`, `prompt.pad.xbox.South`, `prompt.stick.LeftUp`, else a label like `[A]`), `[[` for a bracket; any other tag is an effect (`[wave=3,2]`). Malformed markup is drawn as written and warned once.

| World text | |
|---|---|
| `Text2d` (component, `#[non_exhaustive]`, requires `Transform` and `TextBounds`) | fields `text: RichText`, `style: TextStyle`, `layout: TextLayoutOptions` (anchor `Center`), `lit: bool` (false), `z_order: i32`, `blend: BlendMode`, `opacity: f32` (1.0; applied when drawn, not in the layout cache key) |
| `Text2d` builders | `new(impl Into<RichText>)`, `markup(&str)`, `with_style(TextStyle)`, `with_layout(TextLayoutOptions)`, `with_lighting(bool)`, `with_z(i32)`, `with_blend(BlendMode)`, `with_opacity(f32)` (clamped 0 to 1, a non-number is opaque), `set_text(impl Into<RichText>)` |
| `TextBounds` (component) | `size: Vec2`, `offset: Vec2` (the position to the block's top-left), updated when drawn |
| `TextReveal` (component, `#[non_exhaustive]`) | fields `per_second`, `punctuation_pause`, `fade`; `new(per_second)`, `with_punctuation_pause(f32)`, `with_fade(f32)`, `restart()` (a paused reveal stays paused), `finish()` (shows the rest at the next advance, a fade ended too: a dialogue's Skip; it applies while paused too), `pause()` (hold it where it is: shown characters stay, no more appear or fade in), `resume()`, `is_paused() -> bool`, `is_done() -> bool`, `shown() -> f32`, `drawn_clusters() -> f32` (`shown` plus the run-on that lets a fade finish) |
| `TextRevealed` (message) | `{ entity: Entity }`, written once when a world `TextReveal` has shown every character |

`UiText` (from `ui.text(..)`; it records on `draw() -> Vec2`, which returns its size in UI units, or when dropped, so `ui.text("Hi");` is a statement):

| | |
|---|---|
| `at(x, y)`, `style(&TextStyle)`, `size(f32)`, `color(Color)` | position (UI units) and style |
| `wrap(width)`, `align(TextAlign)`, `max_lines(u32)`, `ellipsis()`, `anchor(Anchor)` | layout |
| `reveal(shown: f32)`, `fade(clusters: f32)` | manual reveal: the first `shown` clusters, fading in over `clusters`; pass `TextReveal::drawn_clusters()` so a fade finishes |
| `typewriter(&mut TextReveal)` | advances the reveal by the UI's real `dt` over this text's layout (`[pause]` and punctuation pauses hold it), once per frame per reveal; sends no message, check `is_done()`; call `restart()` on a new line |
| `opacity(f32)` | fades the whole text, 0 to 1, without a relayout |

Fade text through `opacity` (`UiText::opacity`, `Text2d::opacity`, a tween's alpha target), never through the style colour: colour is in the layout cache key.

---

## UI

Built each frame in `Scene::ui` with `UiContext`: widgets are builders laid out by containers (flexbox and grid), answering from the UI's memory through a `Response`; keyboard and pad navigation work inside focus scopes; the theme is a world resource. The guide is `ryo-ui`. The builder types (`Container`, `Scroll`, `List`, `Button`, `Slider`, ...) live in `ryo2d::core::ui`; games rarely name them.

**Containers** (`.show(|ui| ..) -> Response`; a top-level one is laid out when `show` returns, so its `Response` has this frame's rect, a nested one last frame's):

| `UiContext` | |
|---|---|
| `column()`, `row()`, `grid(columns: u16)`, `stack()`, `panel()` `-> Container` | top to bottom; left to right; equal columns filled row by row; overlapping, placed by anchor and offset; a column with the panel look and padding that blocks the pointer |
| `scroll() -> Scroll` | a column (or row) seen through a view, clipped, scrolled by the wheel, the scrollbar, the right stick and navigation |
| `modal() -> Modal` (`= Container`) | a dialog (panel column, opaque) centred over a dim of the UI, on the popup layer: blocks the pointer, traps focus, returns it when it goes; the game closes it |
| `popup(anchor: UiId) -> Container` | a panel column below `anchor`'s rect (above it when only that fits; centred when unknown), on the popup layer; a trapping focus scope; the game decides when it shows |
| `confirm(title: &str, message: &str) -> Option<bool>`, `confirm_with(title, message, yes: &str, no: &str) -> Option<bool>` | a yes/no modal, No focused first; `Some(false)` on Back; `None` while open; keyed by its texts |
| `scope<R>(key: impl Hash, contents: impl FnOnce(&mut UiContext) -> R) -> R` | key the ids inside without a layout node |
| `node(root: Entity, world: &mut World) -> Response` | draw a node tree spawned with `.embedded()` here, in the open container's flow, focus scope and clip; once a frame (a second call warns); its events arrive after the scene's `ui` returns |
| `scroll_to(UiId)`, `scroll_offset(scroll: UiId) -> Vec2`, `set_scroll_offset(scroll: UiId, Vec2)` | scroll a widget into view from the next frame; read or set (no easing) an area's offset by its `Response::id` |
| `list(count: usize) -> List` | a virtual list: a scroll area exactly as long as its rows that builds only the rows in view and the overscan, each item in a container keyed by its index (`ryo-ui` section 16). Give it a height (`height`, `max_height`, `fill`/`fill_height`, or `grow` in a sized parent): without one it grows to its whole length, cannot scroll, builds only the rows that show and warns once |
| `list_scroll_to(list: UiId, index: usize)`, `list_focus(list: UiId, index: usize)` | scroll the list (its `ListResponse::id`) by the least that shows item `index`'s row; `list_focus` also focuses the row's first focusable widget, up to two frames after it is built. A jump further than a view lands one view short at the list's next draw, then eases. An index past the end is the last item; a list not drawn yet ignores both |

- `Container`: `width(impl Into<Length>)`, `height(..)`, `fill_width()`, `fill_height()`, `fill()`, `min_width(f32)`, `max_width(f32)`, `min_height(f32)`, `max_height(f32)`, `padding(impl Into<Edges>)`, `margin(impl Into<Edges>)`, `gap(f32)`, `align(Align)`, `justify(Justify)`, `center()`, `grow(f32)`, `wrap_items()`, `aspect(ratio)`, `tracks(&[f32])` (grid column widths), `anchor(Anchor)` and `offset(x, y)` (at the top level or in a stack; default `TopLeft`), `at(Rect)` (absolute, even inside a container, keeping its parent's ids, scope and clip), `id(impl Hash)`, `background(Color)` (a fill behind its children), `focus_scope()`, `trap()` and `wrap_focus()` (each a focus scope too), `block_pointer()`, `drag_source::<P>(P)`, `drop_target::<P>()`, `drag_ghost(&Handle<Image>)`, `show(impl FnOnce(&mut UiContext)) -> Response`.
- `Scroll`: `vertical()` (default), `horizontal()`, `both()`, `no_bar()`, and `width`, `height`, `fill_width`, `fill_height`, `fill`, `min_width`, `max_width`, `min_height`, `max_height`, `padding`, `margin`, `gap`, `grow`, `anchor`, `offset`, `at`, `id`, the drag methods and `show(..) -> Response` as on `Container`.
- `List` (vertical): `row_height(f32)` (required: finite and above 0, else the list draws empty, warned once), `columns(u16)` (a grid of equal cells, 1 to 256), `gap(f32)` (between rows and cells), `overscan(usize)` (rows built past each edge of the view, default 2, 1 to 64), `id(impl Hash)`, `width`, `height`, `fill_width`, `fill_height`, `fill`, `min_width`, `max_width`, `min_height`, `max_height`, `padding`, `margin`, `grow`, `anchor`, `offset`, `at`, `no_bar()`, and `show(impl FnMut(&mut UiContext, usize)) -> ListResponse` (the closure builds item `index`). At most 4,096 items are built for the view a frame (warned once). The scroll length is `rows × (row_height + gap) - gap` whatever the rows hold.
- `ListResponse { pub id: UiId, pub range: Range<usize>, pub response: Response }`: the list's id (for `list_scroll_to`, `list_focus`, `scroll_offset`), the items built for the view this frame (the rows in view and the overscan; a focused row kept built out of view is not in it), and the scroll area's response.
- `enum Length { Px(f32), Pct(f32), Fill, Fit }` (default `Fit`; `From<f32>` is `Px`). A fixed-size widget that fills keeps its own size (or its `max_width`/`max_height`, when smaller) as a floor.
- `Edges { left, top, right, bottom }`: `Edges::ZERO`, `all(v)`, `xy(x, y)`, `new(left, top, right, bottom)`, `width()`, `height()`; `From<f32>`, `From<(f32, f32)>`.
- `enum Align { Start, Center, End, Stretch }` (default `Stretch`), `enum Justify { Start, Center, End, SpaceBetween, SpaceAround, SpaceEvenly }` (default `Start`).

**Widgets** (each records on its ending call, or when dropped, so `ui.toggle(&mut on, "Sound");` is a statement):

| `UiContext` | Own builder methods | Ends with |
|---|---|---|
| `button(impl Into<RichText>) -> Button` | `icon(&Handle<Image>)` | `show() -> Response`, `clicked() -> bool` |
| `text(impl Into<RichText>) -> UiText` | see [Text](#text); without `at` it is a layout leaf | `draw() -> Vec2` |
| `image(&Handle<Image>) -> ImageWidget` | `size(w, h)` (without one it is the image's own size: nothing until it is loaded), `tint(Color)`, `filter(Filter)` (`Filter::Default`: nearest in pixel art, else linear), `uv([l, t, r, b])` | `show() -> Response` |
| `checkbox(&mut bool, impl Into<RichText>) -> Checkbox`, `toggle(&mut bool, impl Into<RichText>) -> Toggle` | | `show()` |
| `radio<T: PartialEq + Copy>(&mut T, value: T, impl Into<RichText>) -> Radio<T>` | | `show()` |
| `slider(&mut f32, RangeInclusive<f32>) -> Slider` | `step(f32)`, `show_value()`, `label(impl Into<RichText>)` | `show()` |
| `progress(fraction: f32) -> Progress` | `label(impl Into<RichText>)` | `show()` |
| `text_input(&mut String) -> TextInput` | `placeholder(impl Into<String>)`, `max_len(chars: usize)`, `password()` | `show()` (`changed`, `submitted` on Enter, `focused`) |
| `dropdown(&mut usize, &[&str]) -> Dropdown`, `tabs(&mut usize, &[&str]) -> Tabs` | | `show()` |
| `choice_list(&[&str]) -> ChoiceList` | `default(index)` | `show() -> Option<usize>` |
| `slot() -> Slot` | `icon(&Handle<Image>)`, `count(u32)`, `selected(bool)`, `filter(Filter)` | `show()` |
| `prompt(action: &str) -> Prompt` | | `show()` |
| `rebind(action: &str) -> Rebind` | `label(impl Into<RichText>)`, `slot(usize)` | `show()` (`changed()` the frame it captured) |
| `custom(width: f32, height: f32) -> Custom` | `sense(Sense)` (default `hover`), `takes_horizontal()`, `takes_vertical()`, `takes_confirm(bool)` | `paint(impl FnOnce(&mut Painter, &Response)) -> Response`, `show()` |
| `separator() -> Response`, `spacer() -> Response` | | |

- Shared builder methods (every widget above but `text`, `separator` and `spacer`): `id(impl Hash)`, `enabled(bool)`, `at(Rect)`, `width(impl Into<Length>)`, `height(..)`, `fill_width()`, `margin(impl Into<Edges>)`, `anchor(Anchor)`, `offset(x, y)`, `grow(f32)`, `min_width(f32)`, `max_width(f32)`, `default_focus()`, `nav_up(UiId)`, `nav_down(UiId)`, `nav_left(UiId)`, `nav_right(UiId)`, `nav_skip()`, `focusable(bool)`, `variant(&str)`, `style(LookPatch)`, `tooltip(impl Into<RichText>)`, `drag_source::<P: Clone + Send + Sync + 'static>(P)`, `drop_target::<P: 'static>()`, `drag_ghost(&Handle<Image>)`.
- `Painter` (a custom widget's own units, (0, 0) its top-left): `size() -> Vec2`, `rect(Rect, Color)`, `rounded_rect(Rect, radius, Color)`, `bordered_rect(Rect, fill, radius, border_width, border_color)`, `text(impl Into<RichText>, x, y, &TextStyle)`, `sprite(&Handle<Image>, Rect, tint: Color)`, `line(&[Vec2], width, Color)`.
- `Response { pub id: UiId, pub rect: Rect, .. }`: `hovered()`, `pressed()`, `clicked()` (released over it, or Confirm while focused), `secondary_clicked()`, `double_clicked()` (within 0.3 s), `drag_started()`, `dragging()`, `drag_released()`, `drag_delta() -> Vec2`, `changed()`, `focused()`, `gained_focus()`, `lost_focus()`, `submitted()`, `pointer() -> Option<Vec2>`, `dropped::<T: Clone + 'static>() -> Option<T>`, `drag_hover::<T: 'static>() -> bool`, `drag_started_here()`.
- `Sense`: `hover()`, `click()`, `drag()`, `click_and_drag()`. `UiId` (`Copy`, `Eq`, `Hash`, `Ord`, `Default`): `UiId::NONE`; made from the call site and how often it ran in its container, or `.id(key)`.

**Focus, input and drawing:**

| `UiContext` | |
|---|---|
| `focused() -> Option<UiId>`, `is_focused(UiId) -> bool`, `request_focus(UiId)` (from the next frame), `focus_visible() -> bool` (last device keyboard or pad) | focus (top scene only) |
| `back() -> bool` | Back (Escape, B) pressed and no widget used it; true for the first caller only, in the top scene |
| `nav_step(UiId) -> Option<Direction>` | the direction a focused custom widget that takes it got this frame (with repeat) |
| `held::<P: Clone + 'static>() -> Option<P>` | the drag payload held now |
| `wants_pointer() -> bool` | the pointer is over the top scene's UI, or a widget holds a press |
| `wants_keyboard() -> bool` | a text field or a capturing rebind row has the focus, or an open dropdown list or `ui.popup` is up (not a modal) |
| `input() -> &UiInput`, `consume_key(KeyCode)`, `consume_scroll()`, `into_input() -> UiInput` | the frame's input; consumption carries over to gameplay |
| `draw_rect(Rect, Color)`, `draw_rect_outline(Rect, Color, thickness)`, `draw_rounded_rect(Rect, radius, Color)`, `draw_rect_bordered(Rect, fill, radius, border_width, border_color)`, `draw_panel(Rect, bg, border: Option<Color>)`, `draw_progress_bar(Rect, progress, fill, bg)`, `draw_progress_bar_with_border(Rect, progress, fill, bg, border)`, `draw_modal_overlay(Color)` | absolute shapes; rects are analytic (rounded, bordered, anti-aliased) |
| `draw_sprite(&Handle<Image>, x, y)`, `draw_sprite_scaled(&Handle<Image>, x, y, scale)`, `draw_sprite_rect(&Handle<Image>, Rect, tint)`, `draw_nine_slice(&Handle<Image>, Rect, insets: [f32; 4], tint)` (insets `[left, top, right, bottom]` in texels), `draw_line(&[Vec2], width, Color)` | images and polylines; an image not loaded draws the checker in debug builds only where a rect is given |
| `draw_text(&str, x, y, Color)`, `draw_text_styled(impl Into<RichText>, x, y, &TextStyle)`, `draw_text_anchored(&str, Anchor, offset_x, offset_y, Color)`, `draw_text_anchored_styled(&str, Anchor, offset_x, offset_y, &TextStyle)`, `draw_text_centered(&str, Rect, Color)`, `draw_text_wrapped(&str, x, y, max_width: u32, Color)`, `draw_text_wrapped_centered(&str, Rect, Color)`, `label(&str, x, y, Color, bg: Option<Color>)` | text shorthand (default style: `FontRole::Ui`, 14 px; carry the colour's alpha as opacity) |
| `measure_text(impl Into<RichText>, &TextStyle) -> Vec2`, `line_height(&TextStyle) -> f32`, `text_size(&str) -> (u32, u32)`, `wrapped_text_size(&str, max_width: u32) -> (u32, u32)`, `wrap_text(&str, max_width: u32) -> Vec<String>`, `wrap_text_styled(&str, width: f32, &TextStyle) -> Vec<String>` | measuring and wrapping, in UI units |
| `push_clip(Rect)`, `pop_clip()`, `with_no_clip(f) -> R`, `current_clip() -> Option<Rect>`, `has_clip()`, `clip_depth()`, `clear_clips()`, `point_in_clip(x, y)` | the clip stack (nests and intersects; an empty pop warns once) |
| `dt()` (real time), `set_dt(f32)`, `screen_size() -> (u32, u32)` (UI units), `ui_scale() -> f32` (render pixels per UI unit), `theme() -> &Theme` | |

- Navigation reads the `ui.*` actions (see [Input](#input)) and works only inside focus scopes (`.focus_scope()`, open lists, popups, modals): with no scope on screen the arrows, Space and A stay gameplay's. The engine hides the bindings of every UI action navigation used until they are released. Only the top scene's widgets take input; lower scenes draw and report nothing.
- `UiInput` (the frame's input as the UI sees it): `snapshot(&Input)`, `with_mouse_scale(f32)`, `mouse_position()`, `mouse_x()`, `mouse_y()`, `mouse_down/pressed/released(MouseButton)`, `scroll_delta()`, `key_down/pressed/released(KeyCode)`, `key_repeat(KeyCode)`, `text_input()`, `ime_preedit()`, `ime_preedit_cursor()`, `ime_composing()`, `ime_enabled()`, `gamepad_button_pressed(GamepadButton)`, `left_stick()`, `consume_key`, `consume_mouse`, `consume_gamepad`, `consume_text()`, `consume_scroll()`, `consume_keyboard()`, `is_key_consumed`, `is_mouse_consumed`, `consumed() -> UiConsumed { keys, mouse, gamepad, text, scroll }`, `apply_consumed(&mut Input)`.
- UI tweens (driven by the owner with `update(ui.dt())`): `UiTween<T: Lerp>` with `new(start, end, duration)`, `with_ease(Ease)`, `with_delay`, `with_loop(UiTweenLoop)`, `on_start`, `on_complete`, `update(dt)`, `value()`, `progress()`, `is_finished()`, `is_playing()`, `restart`, `pause`, `resume`, `stop`, `retarget(target)`, `start_value()`, `target_value()`; aliases `TweenFloat`, `TweenVec2` (`(f32, f32)`), `TweenColor`, `TweenRect`; `enum UiTweenLoop { Once, Loop, PingPong, Times(u32) }`.
- `Color { r, g, b, a: u8 }`: `new(r, g, b, a)`, `rgb(r, g, b)`, `hex(0xRRGGBB | 0xRRGGBBAA)`, `with_alpha(a)`, `to_array()`, `to_normalized()`, `to_linear()` (sRGB decoded to linear light); constants `TRANSPARENT`, `BLACK`, `WHITE`, `RED`, `GREEN`, `BLUE`, `YELLOW`, `CYAN`, `MAGENTA`, `GRAY`, `DARK_GRAY`, `LIGHT_GRAY`, `HEALTH_RED`, `MANA_BLUE`, `STAMINA_GREEN`, `XP_GOLD`, `PANEL_BG`, `PANEL_BORDER`.
- `Rect { x, y, width, height }`: `new(x, y, w, h)`, `from_pos_size`, `centered_at(cx, cy, w, h)`, `right()`, `bottom()`, `center()`, `contains(x, y)`, `shrink(p)`, `expand(p)`. `Anchor` is the sprite anchor type (`TopLeft` ... `BottomRight`).

**Theme:**

| | |
|---|---|
| `UiTheme` (resource; `ctx.ui_theme()`, `EngineBuilder::ui_theme(Theme)`) | `new(Theme)`, `get() -> &Theme`, `set(Theme)` (stops following), `modify(impl FnOnce(&mut Theme))`, `follow(Handle<Theme>)` (uses the asset once loaded and again whenever it changes; a failed reload keeps the theme; alone it first applies at the next UI pass, so prefer `ctx.follow_ui_theme`); a change applies from the next frame |
| `Theme { name, palette: Palette, metrics: Metrics, text: TextRoles, widgets: WidgetLooks, variants: BTreeMap<String, Variant>, skins: BTreeMap<String, Skin>, sounds: UiSounds }` | `dark()` (default), `light()`, `pixel()`, `save(impl AsRef<Path>) -> Result<(), ThemeError>`, `look(WidgetKind, variant: Option<&str>, WidgetState) -> UiLook`, `variant(name, Variant) -> Theme`, `skin(name, Skin) -> Theme`; an asset, loaded from `*.theme.ron` by `ThemeLoader` with its skins' images as dependencies |
| `Palette` | `background`, `surface`, `text`, `muted`, `accent`, `danger`, `success`, `warning` |
| `Metrics` | `radius`, `spacing`, `border`, `focus_ring: FocusRing { width, offset, color }` (2, 2), `scroll_line` (40), `scroll_smoothing` (0.08), `repeat_delay` (0.35), `repeat_rate` (0.1), `tooltip_delay` (0.5), `drag_threshold` (4), `double_click` (0.3), `sizes: Sizes` |
| `Sizes` | `button_padding: [f32; 2]`, `button_min_height`, `panel_padding`, `slider: [f32; 2]`, `slider_handle: [f32; 2]`, `toggle: [f32; 2]`, `checkbox`, `scrollbar`, `slot`, `text_input: [f32; 2]` |
| `TextRoles { body, heading, button, caption, mono }` of `TextLook { size, color, font: ThemeFont, bold }` | `TextLook::style() -> TextStyle`; `enum ThemeFont { Ui, Mono, Pixel }` |
| `WidgetLooks` | a `Looks` per kind: `button`, `panel`, `checkbox`, `toggle`, `radio`, `slider`, `text_input`, `dropdown`, `tab`, `choice`, `tooltip`, `scroll`, `slot`, `progress`, `rebind`, `modal`, `prompt`; `get(WidgetKind) -> &Looks` |
| `Looks { normal: UiLook, hovered, pressed, focused, disabled: LookPatch }` | `resolve(WidgetState) -> UiLook`; a patch changes only what it names; pressed builds on hovered |
| `UiLook` (the UI's `Look`) | `fill`, `border`, `border_width`, `radius`, `text`, `padding: Edges`, `offset: [f32; 2]`, `skin: Option<Arc<str>>` |
| `LookPatch` (each field an `Option`) | `fill(Color)`, `border(Color)`, `text(Color)`, `radius(f32)`, `skin(&str)` |
| `Variant { base: WidgetKind, normal, hovered, pressed, focused, disabled: LookPatch }` | `of(WidgetKind)`, `normal(LookPatch)`, `hovered(..)`, `pressed(..)`, `focused(..)`, `disabled(..)` (without one, a disabled widget takes its kind's disabled look); built in: `danger`, `accent`, `tab` (buttons), `selected` (tabs) |
| `Skin { path: Option<String>, insets: [f32; 4], filter: Filter, image: Option<Handle<Image>> }` | `from_path(&str, insets, Filter)`, `from_image(Handle<Image>, insets, Filter)`: a nine-slice drawn in place of a look's fill and border, the fill tinting it |
| `UiSounds { focus, confirm, back, tick, error: ThemeSound, volume_db: f32 }` | what the UI plays per event, through the audio engine on the `ui` bus (one of each per frame at most; `volume_db` added to each sound's level, 0 by default, -60 or less silent); none by default |
| `ThemeSound { path: Option<String>, sound: Option<ThemeSoundSource> }` | `from_path(&str)` (a `.sound.ron` definition, or a clip), `from_sound(Handle<Sound>)`, `from_clip(Handle<AudioClip>)`; a theme file's load with it but are not its dependencies (one that fails is warned once and stays silent); `enum ThemeSoundSource { Sound(Handle<Sound>), Clip(Handle<AudioClip>) }` |
| `enum WidgetKind` | `Button`, `Panel`, `Checkbox`, `Toggle`, `Radio`, `Slider`, `TextInput`, `Dropdown`, `Tab`, `Choice`, `Tooltip`, `Scroll`, `Slot`, `Progress`, `Rebind`, `Modal`, `Prompt` |
| `enum WidgetState` | `Normal`, `Hovered`, `Pressed`, `Focused` (ring showing), `Disabled` |
| `ThemeError` | why `Theme::save` could not write a theme: `Io` (names the path) or `Serialize` |

- A RON theme file is merged over `Theme::dark()` map by map, so it states only what it changes; `Variant::base`, `TextLook::font` and `Skin::filter` are strings there (`base: "Button"`, `font: "Pixel"`, `filter: "Nearest"`).
- `ui.theme()` reads the theme while drawing (`ui.theme().text.heading.style()`, `ui.theme().palette.accent`). Widgets take `.variant(name)` and `.style(LookPatch)`; an unknown variant or skin warns once and draws the plain look.
- UI alpha blends in sRGB space (a 50% black panel over white is mid grey). The console command `debug:ui` outlines every widget with its id.

### Node trees

A UI described once as a `UiNode`, spawned with `ctx.spawn_ui` as one entity per node (`ChildOf` hierarchy), drawn each frame through the builders above before the scene's `ui`. The guide is `ryo-ui` section 15.

| `UiNode` constructors | Spawns with |
|---|---|
| `column()`, `row()`, `grid(columns: u16)`, `stack()`, `panel()`, `scroll()` | |
| `popup(anchor: impl Into<String>)` | a popup below the node of that name in the same tree |
| `text(impl Into<String>)`, `button(impl Into<String>)` | `UiLabel` |
| `image(Handle<Image>)` | `UiImage` |
| `checkbox(label: impl Into<String>, checked: bool)`, `toggle(label: impl Into<String>, on: bool)` | `UiLabel`, `UiChecked` |
| `radio_group(options: &[&str], selected: usize)`, `dropdown(options: &[&str], selected: usize)`, `tabs(options: &[&str], selected: usize)`, `choice_list(options: &[&str])` | `UiOptions`, `UiIndex` |
| `slider(range: RangeInclusive<f32>, value: f32)`, `progress(fraction: f32)` | `UiValue` |
| `text_input(text: impl Into<String>)` | `UiTextValue` |
| `slot()` | `UiSlot` |
| `prompt(action: &str)`, `rebind(action: &str)`, `separator()`, `spacer()` | |
| `confirm(title: &str, message: &str, yes: &str, no: &str)` | a modal panel; buttons with actions `"yes"` and `"no"`; opens on No; Back is a `Clicked` from No |
| `list(count: usize, row_height: f32)` | a virtual list: a scroll area as long as its rows, keeping about a screenful of pooled row entities (each a `UiListRow`) spawned from its `row_shape` and bound by `UiListBind`; its count is state (`ctx.ui_list_count`). Give it a height, as `ui.list` |

| `UiNode` options (each `(self, ..) -> UiNode`) | |
|---|---|
| layout | `width(impl Into<Length>)`, `height(..)`, `fill_width()`, `fill_height()`, `fill()`, `min_width(f32)`, `max_width(f32)`, `min_height(f32)`, `max_height(f32)`, `padding(impl Into<Edges>)`, `margin(impl Into<Edges>)`, `gap(f32)`, `align(Align)`, `justify(Justify)`, `center()`, `grow(f32)`, `wrap_items()`, `aspect(f32)`, `tracks(&[f32])`, `anchor(Anchor)`, `offset(x, y)`, `at(Rect)`, `background(Color)` |
| focus and drag | `focus_scope()`, `trap()`, `wrap_focus()`, `block_pointer()`, `default_focus()`, `nav_up(name: impl Into<String>)`, `nav_down(..)`, `nav_left(..)`, `nav_right(..)` (by node name, resolved in the tree at spawn), `nav_skip()`, `focusable(bool)`, `drag_source()`, `drop_target()` (the payload is the node's entity), `modal()` (a modal root opens centred unless anchored) |
| other shared | `tooltip(impl Into<String>)` (a `UiTooltip`), `enabled(bool)` (`false` adds `UiDisabled`), `variant(&str)` |
| node-only | `action(impl Into<String>)`, `name(impl Into<String>)` (a `UiName`), `hidden()` (a `UiHidden`), `embedded()` (drawn only by `ui.node`), `insert(impl Bundle)` (after the engine's components), `child(UiNode)`, `children(impl IntoIterator<Item = UiNode>)` |
| text | `markup()` (also buttons; text is plain without it), `style(TextStyle)`, `role(TextRole)`, `heading()`, `muted()` (these three read the theme when drawn), `size(f32)`, `color(Color)`, `text_align(TextAlign)`, `wrap(f32)`, `max_lines(u32)`, `ellipsis()`, `typewriter(TextReveal)` (inserts the `TextReveal`) |
| image | `image_size(w, h)`, `tint(Color)`, `filter(Filter)` (also slots), `uv([f32; 4])` |
| button, slot | `icon(Handle<Image>)`; slot: `count(u32)`, `selected(bool)` |
| slider | `step(f32)`, `show_value()`, `label(impl Into<String>)` (also progress and rebind) |
| text input | `placeholder(impl Into<String>)`, `max_len(usize)`, `password()` |
| radio group, scroll | `horizontal()`; scroll: `both()`, `no_bar()` |
| choice list, rebind | `default_choice(usize)`; rebind: `binding_slot(usize)` |
| list | `row_shape(impl Fn() -> UiNode + Send + Sync + 'static)` (required: one row's tree; a file's `row_shape` names a template or is a node), `columns(u16)` (1 to 256), `overscan(u16)` (default 2, 1 to 64), `no_bar()`; `gap` and the size and placement options as above |
| matching and bindings | `key(impl Into<String>)` (what an update or reload matches the node by; defaults to the name, else the kind and position), `bind(impl Into<String>)` (tie a value widget to a named binding), `bound()` (read the node's label and tooltip as `{name}` patterns) |

- An option the node's kind does not take, children given to a widget and `.embedded()` on a child are each warned once at spawn. So are an unknown `nav_*` or popup name, and a name used twice.
- `enum TextRole { Body, Heading, Button, Caption, Mono }`: a theme text role.

| Components (public; the state of a node) | |
|---|---|
| `UiRoot { pub anchor: Anchor, pub offset: Vec2, pub at: Option<Rect>, .. }` | on a root: its placement, which the game may change |
| `UiNodeKind` | every node: `name() -> &'static str` (`"button"`, ...), `action() -> Option<&str>` |
| `UiLabel(pub String)`, `UiImage(pub Handle<Image>)`, `UiChecked(pub bool)`, `UiValue(pub f32)`, `UiIndex(pub usize)`, `UiOptions(pub Vec<String>)`, `UiTextValue(pub String)`, `UiTooltip(pub String)` | state components; the walk writes a value only when the player changes it |
| `UiSlot { pub icon: Option<Handle<Image>>, pub count: u32, pub selected: bool }` | a slot's contents |
| `UiName(pub String)`, `UiHidden`, `UiDisabled` | markers: insert or remove to name, hide (with its subtree) or disable |
| `UiInteraction` | every node, as of its last draw: `hovered() -> bool`, `focused() -> bool`, `id() -> Option<UiId>` |
| `UiListRow` | each pooled row of a list node: `index() -> usize`, the item it shows. A row is its own binding and naming scope (`ui_set(row, ..)`, `find_ui_in(row, ..)`) |

| Events | |
|---|---|
| `UiEvent { pub node: Entity, pub name: Option<String>, pub action: Option<String>, pub kind: UiEventKind, .. }` | a `Message` (read with `ctx.ui_events()`) and an `EntityEvent` on its node (`ctx.world.entity_mut(node).observe(\|e: On<UiEvent>\| ..)`; observers run while the scene stack is out of the world: they must not change scenes) |
| `UiListBind { pub list: Entity, pub row: Entity, pub index: usize }` | a list node gave a pooled row an item: fill it from the game's data. A `Message` (`ctx.messages::<UiListBind>()`; every list's reach every scene, so keep to `list`) and an `EntityEvent` triggered on the row. Sent at the start of the UI pass for each new or reused row (and again on `ui_list_refresh`); a new or reused row is drawn from the next frame |
| `enum UiEventKind { Clicked, SecondaryClicked, Changed, Submitted, Dropped { source: Entity }, Back, FocusGained, FocusLost }` | buttons, images, slots, prompts, progress and containers send `Clicked`; value widgets and rebind rows `Changed`; a choice list `Clicked` (the pick in `UiIndex`); a modal or popup node `Back` |

- **Cached.** A node whose inputs did not change replays last frame's build; a node rebuilds, with its ancestors, when its components change (so `ui_set`, `update_ui` and reloads), when the player interacts with it, while it is live (scroll areas, lists, a typewriter), when something it read changed, or when the theme, fonts, UI size or scale, prompts or top scene change. A write through `bypass_change_detection` is not seen. Closure UI is not cached. The guide is `ryo-ui` section 15, Cached node trees.

### UI files

A screen written in a `.ui.ron` file loads into the same `UiNode`. The guide is `ryo-ui` section 15, Files.

| Type | |
|---|---|
| `UiFile` | an asset (`.ui.ron`, `UiFileLoader`), checked when it loads, its imports and images loaded with it: `ctx.assets().load_now::<UiFile>(path)`, or `parse(name: &str, text: &str) -> Result<UiFile, UiFileError>` for text the game holds, then `ctx.assets().add(file)` (its imports load at its first build); `path() -> &str` |
| `Handle<UiFile>` | `root(&self, assets: &mut AssetsView<'_>) -> Result<UiNode, UiFileError>`, `template(&self, name: &str, args: &UiArgs, assets: &mut AssetsView<'_>) -> Result<UiNode, UiFileError>` (pass `&mut ctx.assets()`): build the root or an instance of a template; the tree holds the file, and follows its reloads |
| `UiFileError` (`pub issues: Vec<UiFileIssue>`) | every mistake found; `Display` prints one `path:line:column: message` per line. In a failed load's `AssetError::Decode`, which names the file, the file's own mistakes have an empty `path` and print as `line:column: message`, and an import's keep theirs. `From<Arc<AssetError>>` (every mistake given its path), so `?` turns a failed load into one |
| `UiFileIssue { pub path: String, pub line: u32, pub column: u32, pub message: String }` | one mistake; lines and columns count from 1, line 0 means no place in the text |
| `UiArgs` | a template's arguments: `new()`, `with(self, name: &str, value: impl Into<UiArg>) -> UiArgs`; giving a name again replaces it. A name is a parameter or any option of the template's root node (`.with("default_focus", true)`); a reload applies them again, while options set in code on the built node are lost |
| `enum UiArg { Number(f64), Text(String), Bool(bool), List(Vec<UiArg>) }` | `From` for `f32`, `f64`, `i32`, `i64`, `u32`, `u64`, `usize`, `bool`, `&str`, `String`, `Vec<T: Into<UiArg>>` and `&[&str]` |
| `enum BindValue { Number(f64), Text(String), Bool(bool) }` | a binding's value; `From` for `f32`, `f64`, `i32`, `i64`, `u32`, `u64`, `usize`, `bool`, `&str` and `String`, `TryFrom` back out |
| `UiReloaded { pub root: Entity, pub replaced: Option<Entity> }` | a `Message`, sent once per root whose tree a reload brought in line, before that frame's UI pass; `replaced` is the old root when the reload gave the root a new kind (a new entity), `None` when it was updated in place |

- A scene's trees are walked before its `ui`, so `ui` and `update` read their events in the same frame. An embedded tree's events are delivered after `ui` returns.
- Ids are keyed by entity under the parent's id: widget state survives sibling changes and is forgotten on despawn.

---

## Errors

| Type | Variants | Alias |
|---|---|---|
| `EngineError` | `Window(String)`, `Render(String)`, `WorkspaceError(String)`, `Gpu(GpuError)` | `EngineResult<T>` |
| `GpuError` | `NoAdapter`, `RequestDevice`, `CreateSurface`, `DeviceLost` (`String`) | |
| `AssetError` | `NotFound`, `Io`, `Decode`, `NoLoader`, `Dependency`, `Cycle`, `InvalidPath` (see [Assets](#assets)); shared as `Arc<AssetError>` | |
| `ImageError` | `Decode(image::ImageError)` (from `Image::decode`) | |
| `MusicJoinError` | `StreamedIntro`, `StreamedLoop`, `Rates { intro, looped }`, `EmptyLoop` (from `Music::intro_and_loop`; a failed audio file is an `AssetError`) | |
| `AudioClipError` | `ZeroSampleRate` (from `AudioClip::from_frames`) | |
| `SaveError` | `Io { path: String, source: io::Error }`, `Corrupt { slot: String, reason: String }`, `NewerVersion { found: u32, current: u32 }`, `NotFound(String)`, `InvalidSlot(String)`, `StorageFull`, `Unavailable`, `Serialize(Box<dyn Error + Send + Sync>)`, `Deserialize { slot: String, source: Box<dyn Error + Send + Sync> }` (see [Config, settings and saves](#config-settings-and-saves)); shared as `Arc<SaveError>` in messages | |
| `NetworkError` | `InitError`, `ConnectionError`, `SendError`, `ReceiveError`, `TransportError` (`String`) | `NetworkResult<T>` |
| `ReplicationError` | `SerializationError`, `DeserializationError` (`String`), `TypeMismatch { expected, found }`, `UnknownComponent(ComponentTypeId)`, `EntityNotFound(u64)` | `ReplicationResult<T>` |

All are `thiserror` enums (`Display`, `Error`); `GpuError` converts into `EngineError` with `?`.

---

## Navigation

Grid A* plus steering; the game moves its agents.

- `NavigationGrid`: `new(world_w, world_h, cell_size)`, `with_origin(w, h, cell, ox, oy)`, `for_layer(LayerGrid, window: CellRect)` (a window, max exclusive, of a tile layer's own cells on any grid: `GridCoord`s are the layer's cells, signed, and cells outside the window are `Blocked`; six neighbours on hex (`MapGrid::neighbours`), the eight of the square lattice elsewhere (a staggered map's diamonds' lattice), a diagonal needing both cells beside it walkable; a step costs the world distance between the cells' centres over the shortest step, times the entered cell's cost, so isometric paths take their screen distance; the heuristic and the tie-break measure world centres the same way; line of sight follows the grid; world positions go through the layer's whole transform; a window over 4,194,304 cells keeps the middle 2048 cells of each longer side, logged once a run; `MapsView::navigation` makes one with a map's collision blocked), `window() -> CellRect` ((0, 0)..(width, height) for `new`), `layer() -> Option<LayerGrid>`, `floor() -> Floor` (the storey whose cells it holds, which its paths' nodes carry: `GROUND` unless made for another), `on_floor(self, Floor) -> NavigationGrid` (puts it on a storey, its cells unchanged), `dimensions()`, `cell_size()` (a layer grid's shortest step), `generation()`, `world_to_grid(x, y) -> GridCoord`, `grid_to_world(GridCoord)` (cell centre), `grid_to_world_corner(..)` (top-left of the cell's box), `in_bounds`, `get`/`set(GridCoord, CellState)`, `is_walkable`, `cost`, `neighbors`, `line_of_sight(a, b)`, `clear()`, `block_rect(min_x, min_y, max_x, max_y)` (on a layer grid every cell whose outline meets the rectangle, edges included), `block_collider(&GlobalTransform, &Collider, Entity)`, `block_tiles(&TileColliders, to_world: Affine2)` (`to_world` the tiles' entity's whole world transform, `global.affine()`; the solid and slope cells, and shape cells whose solid shape covers the centre, negative ones included, reading only the cells of the chunks under the grid, after a look at every chunk to filter them; one-way cells stay walkable; a layer grid's own tiles, within 1e-4 of its transform, block cell for cell; on a square grid, tiles that stay upright rectangles block the cells they cover; any other tiles, diamonds, skewed, turned or hexagons, block every cell whose outline shares area with theirs, a right-angle turn read exactly; tiles by a transform scaled to nothing or not finite block nothing), `rebuild_from_world(&mut World)` (every non-trigger collider, and tile maps through `block_tiles` with their `GlobalTransform` instead of their outlines; a floor's edge, `MapFloor`, blocks nothing). `Clone`.
- `FloorNavigation` (`Clone`): a grid per storey over the same cells, joined at stair cells. `new(grids: Vec<NavigationGrid>, links: Vec<StairLink>)` (the grids by their `floor()`, the first for each storey number kept; a grid on `Every`, and a link between storeys without grids or from a storey to itself, left out; the grids must cover the same cells, as `MapsView::floor_navigation`'s do, or the search's heuristic, measured on the lowest storey's grid, does not hold), `grid(Floor) -> Option<&NavigationGrid>`, `grids()` (lowest first), `links() -> &[StairLink]` (by row). `StairLink { cell: GridCoord, lower: Floor, upper: Floor }`: stairs joining two storeys at one cell, crossed for a thousandth of a step (next to nothing, but enough that of paths otherwise as cheap the search takes the one changing storey least).
- `NavigationBuilder::new(world_w, world_h).with_cell_size(f32).with_origin(x, y)`, then `build()` or `build_from_world(&mut World)` (blocks every collider's cells).
- `enum CellState { Walkable, Blocked, Difficult(f32) }` (`cost()`, `is_walkable()`); `GridCoord { x: i32, y: i32 }` (`new`; `From<IVec2>`, and `IVec2: From<GridCoord>`).
- `Pathfinder`: `new()`, `with_config(PathfinderConfig)`, `config()`, `find_path(&self, &grid, sx, sy, gx, gy) -> PathResult`, `find_path_grid(&self, &grid, GridCoord, GridCoord)`, `find_floor_path(&self, &FloorNavigation, start: Vec2, start_floor: Floor, goal: Vec2, goal_floor: Floor) -> PathResult` (ground-space points, where characters stand, each through its storey's grid; a click on a storey's art goes through `MapsView::floor_cell_at_world` into the grid form), `find_floor_path_grid(&self, &FloorNavigation, (Floor, GridCoord), (Floor, GridCoord))` (the same A* over every storey's cells, a link's step costing a thousandth of a step, so a path crosses a run of stairs once; the heuristic and tie-break on the lowest storey's grid; `max_total_nodes` for the whole search; an end on a storey without a grid, outside its grid or blocked is `InvalidEndpoints`; smoothing within each storey's run, keeping the two nodes at each link; synchronous only: no time-sliced search across storeys), `start_pathfinding(&mut self, &grid, sx, sy, gx, gy) -> (u64, PathResult)`, `continue_pathfinding(&grid, id)`, `cancel_pathfinding(id)`, `active_request_count()`.
- `PathfinderConfig { max_nodes_per_frame, max_total_nodes, heuristic_weight, default_smoothing, tie_breaking }`: `with_time_slicing(n)`, `with_smoothing(PathSmoothing)`, `with_heuristic_weight(w)`.
- `enum PathResult { Found(Path), InProgress, NoPath, InvalidEndpoints, TooExpensive }`: `is_found()`, `path() -> Option<Path>`, `is_in_progress()`.
- `Path`: `new(waypoints, generation)`, `empty()`, `is_valid()`, `is_complete()`, `len()`, `waypoints()`, `current_waypoint()`, `destination()`, `advance()`, `reset()`, `total_length()`, `remaining_length()`, `is_stale(grid_generation)`, `smooth_line_of_sight(&grid)` (only the runs of nodes on the grid's storey, each on its own), `smooth_spline(segments)` (within each storey's run, keeping its ends). `PathNode { x, y, grid_coord, floor: Floor }` (`new(x, y, grid_coord)` on `GROUND`; a path across storeys changes `floor` between two nodes at one stair cell). `enum PathSmoothing { None, LineOfSight, Spline }`.
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

- `profile_function!()` and `profile_scope!("name")`: puffin scopes, recorded only in a build with the engine's `profiler` feature (`ryo2d dev -p`, which passes `--features ryo2d/profiler`; the game needs no feature of its own) and while scopes are on (the profiler window open, or `[debug] enable_profiler`); nothing without the feature. See `ryo-profiler`.
- `FrameCost` (resource the engine writes at the end of every frame, windowed or headless; read it with `ctx.resource::<FrameCost>()`, never write it): what recent frames cost the CPU, smoothed over about 50 frames, as the F1 frame timing overlay shows it. `update_ms()` (the UI pass and the fixed steps: scenes, systems, physics; the other phases, such as the asset pump, map spawning and transform propagation, count only in the total), `render_ms()` (gathering, building and uploading the frame), `present_ms()` (running the render passes and presenting, without the wait for the window's next image), `total_ms()` (the frame's work, the vsync wait and the sleep between frames left out), `fps()` (from wall-clock time, waits included; 0 until the second frame), `frames()` (frames measured). For a game's own performance readout.
- `DEV_TOOLS: bool` (prelude; `ryo2d::DEV_TOOLS`): whether this build has the developer tools (the console, the dev windows, the debug keys and overlays, the log's ring): true in a development build (debug assertions) or with the `dev-tools` feature (`ryo2d build --dev-tools`), false in a game built with `ryo2d build`.
- Logging (prelude macros): `trace!`, `debug!`, `info!`, `warn!`, `error!`, `format!`-style (`info!("score {}", s)`), logged under the target `game::` and the module path (`game::my_game::ai`); `warn_once!(..)` logs the first time its call site is reached with warnings let through (a site reached while its target is silenced keeps its line for later), for a warning in code that runs every frame. Usable before the engine is built (the first call installs the logger). Default levels: `info` for the game and the engine, `warn` for other crates; a game calling `log::info!` itself logs as a library (hidden at the defaults). Output: the terminal in a development build and every headless run, the browser console on the web, the log file (`logs/latest.log` in the save folder, the last run's `previous.log`, for every windowed native run), the logger window (F9) and the console. See `ryo-game-dev`, Logging.
- `ryo2d::logging` (not in the prelude): `set_level(target: &str, LevelFilter)` and `level(target: &str) -> LevelFilter` (`game`, `engine` and `library` name the three defaults; any other target covers the targets under it, and the nearest wins), `ensure()` (install the logger; the macros and the engine call it), `Level` (`Error`, `Warn`, `Info`, `Debug`, `Trace`) and `LevelFilter` (those and `Off`; the `log` crate's, re-exported), `LogRecord { seq: u64, level: Level, target: String, origin: Origin, message: String, time: Duration, frame: Option<u64>, scope: u64 }` (`#[non_exhaustive]`: read, never built), `Origin { Engine, Game, Library }` (`Origin::of(target)`). `RYO_LOG` sets levels at start (`RYO_LOG=warn,game=debug`; `ryo-cli`).
- Console (backquote key; a developer tool, absent from a game built with `ryo2d build`, where every call below compiles and does nothing): `EngineBuilder::console_command(spec, |world: &mut World, cmd| -> Result<(), String>)` (for the run; replaces a built-in of the same name) and `remove_console_command(name)`; a scene's `ctx.console()` (`ConsoleScope`): `command::<S>(spec, |scene: &mut S, ctx, cmd| -> Result<(), String>)` (leaves with the scene; the uppermost scene's answers), `remove(name) -> bool`, `print(text)`, `print_colored(text, Color)`. `CommandSpec { name, description, usage: Option<String>, help: Option<String> }` (`#[non_exhaustive]`): `CommandSpec::new(name, description).usage(..).help(..)` (or a `(&str, &str)` pair); names are lower-cased, and one that is empty or has a space is refused with a warning. `Command`: `name() -> &str`, `args() -> &[String]`, `arg::<T: FromStr>(i) -> Result<T, String>` (the error names the argument and the usage), `arg_or::<T>(i, default)`, `rest(i) -> String`, `print`, `print_colored`, `act(SceneAction)` (applied after the handler: a builder command's always, a scene's only from the top scene), `quit()`; the `Err` prints in red. See `ryo-console`.
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
| `assets() -> AssetsView<'_>` | the asset registry between steps; loads queued with `load` land in a step's pump |
| `saves() -> MemorySaves` | the in-memory store a headless engine keeps its saves and `system.json` in (never the disk); `EngineBuilder::memory_saves(store)` gives one before the engine starts |
| `MemorySaves` (`Clone`: the same store, `Default`) | `new()`, `put(name: &str, bytes: impl Into<Vec<u8>>)`, `get(name: &str) -> Option<Vec<u8>>`, `names() -> Vec<String>`, `remove(name: &str)`: files by name (`slot1.ryo-save`, `system.json`), to plant an old or damaged save and read what the game wrote |
| `press_key` / `release_key` / `tap_key` / `type_text(&str)` | keyboard input (`KeyCode`) |
| `press_mouse` / `release_mouse` / `mouse_move(x, y)` | mouse input (screen space) |
| `mouse_move_window(x, y)` | the pointer at physical window pixels, mapped like a real cursor |
| `press_pad(GamepadButton)` / `release_pad` / `tap_pad`, `set_stick(Stick, x, y)` (-1..1, +y up, held until set again), `wheel(dx, dy)` (lines), `trackpad(dx, dy)` (logical pixels) | pad, wheel and trackpad input; a virtual pad connects if none is |
| `ui_focused() -> Option<UiId>`, `ui_focusables() -> Vec<(UiId, Rect)>`, `focus_ui(UiId)` | the focus the last step resolved; what the D-pad can land on in the top scene (focusable, enabled, shown, in a scope, not `nav_skip`, inside the innermost trap; rects in UI units); a focus request for the next step |
| `StepOutcome` | `Running` \| `Quit` |
| `HeadlessError` | `NoGpu` \| `Render(String)` \| `Capture(CaptureError)` |
| `shared_gpu()` | one GPU device shared across tests |
| `ryo2d::testing::capture_log() -> LogCapture`, `capture_log_bounded(limit: usize) -> LogCapture` | free functions: collect the records logged on this thread, and by the threads of an engine, world or asset server built after it, until it drops (open it first; other tests' records never reach it); its records skip the terminal, and are printed if the test fails. `_bounded` keeps the newest `limit` (for a flood). Drop it on the thread that opened it |
| `LogCapture` | `records() -> Vec<LogRecord>`, `count(ryo2d::logging::Level) -> usize` (exactly that level), `warnings() -> Vec<String>`, `warnings_and_errors() -> Vec<String>`, `containing(&str) -> usize`, `containing_all(&[&str]) -> usize`, `from_target(prefix: &str) -> Vec<LogRecord>` (the target or under it, by whole `::` segments: `game` takes `game::menu`, not `gamepad`), `seen() -> u64` (every record, those a bounded capture dropped too), `scope() -> u64` |
| `HeadlessEngine::console_run(line: &str) -> Vec<String>` | run a console line as if typed; the lines it printed, its error last. In every build: without the tools (a release test run) it runs nothing and returns an empty `Vec` |
| `HeadlessEngine::audio_log() -> Vec<AudioLogEntry>` | doc-hidden, for tests: what the engine did with each voice and bus (the latest 4096); `AudioLogEntry { kind: AudioLogKind, voice: u32, sound, source, bus: String, gain_db: f32, pan: f32, rate: f64, fade: Duration, reason: &'static str }`, `AudioLogKind { Start, Stop, Pause, Resume, Gain, Pan, Rate, Refused, Culled, Stolen }` (`ryo2d::core::audio`; `voice` 0 is a refused play or a bus's change; `reason`, on `Refused` only, names the rule first: `per_frame`, `cooldown`, `limit`, `full`, `undecodable`, `locked`, `unlock` or `loading`). Headless audio plays silently on kira's mock backend, its picks seeded |
| `ryo2d::testing::audio_world() -> World`, `audio_world_over(files: MemorySource) -> World`, `audio_step(world: &mut World, dt: Duration)` | free functions: `asset_world()` plus a clock and the audio engine on the mock backend, its log on (`world.resource::<ryo2d::core::audio::AudioEngine>().log()`, doc-hidden); `_over` reads the test's `MemorySource`, to save over a file and reload it; `audio_step` is one frame (clock, pump, audio). Seeded as a headless engine is; `AudioView::new(&mut world).seed(n)` for another sequence |
| `ryo2d::testing::asset_world() -> World` (a free function, not a method) | a world with the asset registry and no engine (every engine store, over an empty `MemorySource`), for a test that needs handles: `world.resource_mut::<Assets<Image>>().add(image)` |
| `UiHarness::new(width, height)` | a UI with no window or GPU (built-in fonts, scale 1, the dark theme) for unit-testing game UI; `with_theme(Theme)` |
| `UiHarness::frame(\|ui\| ..) -> R` | one 60 fps frame: resolve the scripted input against the last frame, run the closure as the top scene's `ui`, finish the frame |
| `UiHarness::move_to(x, y)`, `press()`, `release()`, `wheel(dx, dy)`, `trackpad(dx, dy)`, `action(&str)` | script the next frame: the pointer (UI units), the left button (held from `press` to `release`), the wheel (lines), a trackpad (logical pixels, followed at once), a UI action (`ui.up`/`down`/`left`/`right`/`confirm`/`back`/`next`/`tab_next`/`tab_prev`; another name warns once and does nothing) |
| `UiHarness::focused() -> Option<UiId>`, `command_count() -> usize` | the focus after the last frame; the UI commands it recorded |
| `UiHarness::spawn_ui(UiNode) -> Entity`, `world() -> &World`, `world_mut() -> &mut World`, `find_ui(&str) -> Option<Entity>`, `ui_id(Entity) -> Option<UiId>`, `events() -> &[UiEvent]` | node trees in the harness: each `frame` draws them before its closure; `events` are the last frame's, in walk order |
| `UiHarness::frame_with_world(\|ui, world\| ..) -> R` | `frame`, with the world too (for `ui.node(root, world)`) |
| `UiHarness::update_ui(node, tree) -> Entity`, `ui_set(node, name, value)`, `ui_get::<T>(node, name) -> Option<T>` | as on `SceneContext` |
| `UiHarness::ui_list_count(list, count)`, `ui_list_refresh(list)`, `ui_list_refresh_row(list, index)`, `ui_list_scroll_to(list, index)`, `ui_list_focus(list, index)`, `ui_list_index(node) -> Option<usize>` | list nodes, as on `SceneContext`; read a frame's binds with `messages::<UiListBind>()` |
| `UiHarness::with_asset_source(impl AssetSource) -> UiHarness`, `assets() -> AssetsView<'_>`, `spawn_ui_file(path) -> Result<Entity, UiFileError>`, `reload_ui(path) -> Result<(), Arc<AssetError>>`, `messages::<M>() -> impl Iterator<Item = M>` | the harness's asset registry, over an empty `MemorySource` unless given a source (a `DiskSource` over a folder, or the test's `MemorySource`); each `frame` runs the pump first. `reload_ui` reads a UI file again and waits until the reload lands, so the next `frame` brings its trees in line, and `messages::<UiReloaded>()` lists that frame's reloads |
| `UiHarness::set_theme(Theme)`, `resize(width, height)`, `type_text(&str)` | from the next frame: draw with another theme, change the UI's size (UI units), type text into the focused field |
| `UiHarness::set_ui_cache(bool)`, `set_ui_fragments(bool)`, `set_ui_spot_check(bool)`, `ui_cache_stats() -> UiCacheStats` | the node cache from the next frame: on (default) or off (every node built, as `EngineBuilder::ui_cache(false)`); fragments on (default) or off (unchanged subtrees replayed, as `EngineBuilder::ui_fragments(false)`); the debug spot check, off in the harness (the engine runs it in debug builds; release builds have none); what the last frame's walk did |
| `UiCacheStats { pub built: usize, pub replayed: usize, pub rebuilt: Vec<Entity>, pub spot_checked: Option<Entity>, pub records: usize, pub reused: usize, pub resolved_again: usize, pub captured: usize, pub spot_fragment: Option<UiId>, pub spot_fragment_differed: usize, pub spot_kept: usize }` | the nodes built, the tree nodes replayed (an open dropdown is its trigger, list and options), the nodes built in walk order, the node the spot check built in place of its replay (not in `built`), the records the cache keeps; the unchanged subtrees copied from their kept fragments, the kept fragments resolved again (moved, resized or under another clip), the subtrees replayed and kept; the kept fragment the spot check resolved in place of its copy (by its root's id), those it found differing (never, while every input a fragment's output follows from is checked) and those the spot checks captured afresh and found the same (not in `reused`) |
| `UiHarness::frame_snapshot() -> String`, `HeadlessEngine::ui_snapshot() -> String`, `HeadlessEngine::ui_cache_stats() -> UiCacheStats` | doc-hidden, for comparing two runs: the last frame's UI in `Debug` form (the commands that can show with their scroll slots, records, focus, events, widget states, every node's components); equal with caching or fragments on and off |
| `HeadlessEngine::ui_render_stats() -> UiRenderStats`, `ui_runs_snapshot() -> String`, `ui_scroll_slots() -> Vec<(Option<Rect>, [i32; 2])>`, `set_ui_spot_check(bool)`, `PixelPair::new(make)`, `PixelPair::fragments(make)` | doc-hidden, for the engine's tests: the last frame's render step (`UiRenderStats { kept, texts_built, texts_reused, glyph_evictions, runs_copied, slot_runs_copied, runs_built, instances_written }`), its fragment runs in `Debug` form (what `ui_snapshot` leaves out), each scroll slot's clip and drawn origin (whole render pixels); the debug spot check on or off; two engines from one builder maker compared pixel for pixel after every frame, retention on against off, or fragments on against the node cache off |
