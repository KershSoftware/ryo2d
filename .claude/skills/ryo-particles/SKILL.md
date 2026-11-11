---
name: ryo-particles
description: Particles - ParticleEffect assets built in code or read from hot-reloading .particles.ron files (emission, shapes, curves and gradients, sprite sheets, forces, sub-emitters), ParticleEmitter entities the engine runs and draws, one-shots with ctx.particles().play_at, attractors, collisions, presets, pause and time, the budget.
metadata:
  version: 2.3.0
---

# Ryo2D Particles

You help users create particle effects (fire, smoke, explosions, magic, weather) with effects, emitters, attractors and collisions.

**For complete API signatures, see the `ryo-api` skill.**

## Effects and emitters

An **effect** says how a kind of particle is born, moves, looks and ends: a `ParticleEffect` asset, built in code or read from a `.particles.ron` file. An **emitter** places one in the world: an entity with a `ParticleEmitter`. The engine spawns, moves, collides and draws the particles of every emitter; a game calls nothing each frame.

```rust
#[derive(Default)]
struct Level {
  boom: Option<Handle<ParticleEffect>>,
}

impl Scene for Level {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // An asset path, relative to assets/ (see ryo-assets)
    match ctx.assets().load_now::<Image>("particle.png") {
      Ok(spark) => {
        // A fire that burns where its entity stands, until it is despawned
        let fire = ctx.assets().add(ParticleEffect::fire(spark.clone()));
        ctx.world.spawn((Transform::from_xy(400.0, 500.0), ParticleEmitter::new(fire)));
        // For the bursts played later
        self.boom = Some(ctx.assets().add(ParticleEffect::explosion(spark, 50)));
      }
      Err(error) => warn!("no particle sprite: {error}"),
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if ctx.input.mouse_pressed(MouseButton::Left) && let Some(boom) = &self.boom {
      let (x, y) = ctx.input.mouse_position();
      let at = ctx.camera().screen_to_world(Vec2::new(x, y));
      ctx.particles().play_at(boom, at);
    }
    SceneAction::Continue
  }
}
```

- An effect holds its image and an emitter holds its effect: what an emitter plays stays loaded while the emitter does. Many emitters can share one effect (the `Handle` clones cheaply). An image that is not loaded draws the checker in debug builds and nothing in release ones.
- Presets and `ParticleEffect::new` take the image's handle by value, so clone it for each effect.
- Each particle is a world-space quad centred on its position, `size` world units across: every camera draws it through its own view, so it zooms, turns and shakes with the world.
- Angles in effects are **degrees**, and their fields end in `_deg`: 0 is right and 90 is down (the rest of the engine is in radians).

### Presets

Constructors on `ParticleEffect`, to place as they are or to change with the builders below.

| Preset | Use |
|--------|-----|
| `fire(image)`, `flame_small(image)` | Torches, bonfires |
| `smoke(image)`, `smoke_thick(image)` | Ambient smoke |
| `explosion(image, count)`, `explosion_debris(image, count)` | One burst, played once |
| `magic(image)`, `magic_heal(image)` | Sparkles |
| `rain(image)`, `snow(image)` | Weather |
| `sparks(image)`, `dust(image)` | Impacts, one burst, played once |
| `confetti(image)` | Celebration, one burst, played once |
| `bubbles(image)` | Underwater |
| `comet_trail(image)`, `exhaust(image)` | Projectile trails, engines |

A preset played once is finished when its burst has died: play it with `ctx.particles().play_at`, or its emitter stays behind, finished, until something despawns it.

## Building an effect

<!-- given: sprite: Handle<Image> -->
```rust
ParticleEffect::new(sprite)
  .with_rate(30.0)
  .with_shape(EmitShape::Circle { radius: 10.0, arc_deg: 360.0, thickness: 1.0 })
  .outward()
  .with_lifetime((1.0, 2.0))
  .with_size((4.0, 8.0))
  .with_size_over_life(Curve::new([(0.0, 1.0), (1.0, 0.25)]))
  .with_color_over_life(Gradient::new([(0.0, Color::CYAN), (1.0, Color::TRANSPARENT)]))
  .with_speed((20.0, 60.0))
  .with_gravity(0.0, -20.0)
  .with_drag(0.5)
  .with_max_particles(300)
```

An effect has public fields with these builders. A spawn value is a number or a random `(min, max)` (`impl Into<ValueRange>`): a min above its max samples the min.

### Emission

| Builder | Default | What |
|---------|---------|------|
| `with_rate(f32)` | 10 | Particles per second, on top of the bursts |
| `with_rate_per_distance(f32)` | 0 | Particles per pixel the emitter moves, on top of the rate; a `World` effect's spread along the move (none across a teleport) |
| `with_burst(Burst)` | none | Particles released at a moment of each cycle |
| `with_duration(f32)` | 1 s | One cycle |
| `with_looping(bool)` | true | Whether the cycle repeats |
| `with_start_delay(f32)` | 0 s | Before the first cycle |
| `with_prewarm(bool)` | false | Starts a looping effect as if it had played a cycle already (at most 10 s of it, run at once in the emitter's first step; it needs `looping`) |
| `with_max_particles(u32)` | 1000 | Most live particles one emitter keeps |

<!-- given: sprite: Handle<Image> -->
```rust
ParticleEffect::new(sprite.clone()).with_rate(30.0); // a stream, on and on
ParticleEffect::new(sprite.clone()).with_rate(0.0).with_burst(Burst::at(0.0, 50.0)).with_looping(false); // one burst
// 5 bursts, 0.5 s apart: a burst fires only within its cycle, so the cycle lasts 2.5 s
ParticleEffect::new(sprite.clone()).with_rate(0.0).with_burst(Burst::at(0.0, 20.0).with_cycles(5, 0.5)).with_duration(2.5).with_looping(false);
// A burst count can be a random range; 0 cycles repeats until the emitter stops
ParticleEffect::new(sprite).with_burst(Burst::at(0.25, (2.0, 4.0)).with_cycles(0, 0.1));
```

Set `with_rate(0.0)` on a burst-only effect: the default rate is 10 a second. An effect that does not loop ends with its cycle: its emitter stops and its live particles finish.

A stream from a moving emitter spreads its spawns along the step's move, each aged by the part of the step left, so a fast emitter leaves an even line. This holds for a `World` effect; a `Local` effect's particles are born at the emitter.

### Shape and direction

| Builder | Default | What |
|---------|---------|------|
| `with_shape(EmitShape)` | `Point` | Where particles are born |
| `with_direction(direction_deg, spread_deg)` | 0, 360 | Direction of travel, and the whole width of the cone round it (360: any direction) |
| `outward()` | off | Travel away from the shape's centre instead |

<!-- given: sprite: Handle<Image> -->
```rust
EmitShape::Point;
EmitShape::Line { length: 100.0 }; // horizontal, centred on the emitter
EmitShape::Circle { radius: 20.0, arc_deg: 360.0, thickness: 0.0 }; // thickness 0: the rim only, 1: the whole disc
EmitShape::Rect { size: Vec2::new(800.0, 10.0), thickness: 1.0 }; // thickness 0: the outline, 1: the whole rectangle
ParticleEffect::new(sprite).with_direction(-90.0, 34.0).with_speed((50.0, 120.0)); // a cone, upward
```

The shape turns and scales with the emitter's transform. The emitter's scale moves the shape only, never a particle's size or speed. A mirrored scale mirrors directions, and a zero scale emits from the centre.

### Spawn values and life

| Builder | Default | What |
|---------|---------|------|
| `with_lifetime(range)` | (1, 2) s | How long each particle lives |
| `with_speed(range)` | (0, 50) | Starting speed |
| `with_size(range)` | (8, 16) | Starting size, in world units |
| `with_rotation(range)` | 0 | Starting rotation, in degrees |
| `with_angular_velocity(range)` | 0 | Spin, in degrees per second |
| `with_color(Color or (Color, Color))` | white | Starting tint; a pair picks a random mix of the two |
| `with_inherit_velocity(f32)` | 0 | The share of the emitter's velocity a particle starts with, on top of its own (`World` effects only) |
| `with_color_over_life(Gradient)` | white to transparent | Multiplies the starting tint |
| `with_size_over_life(Curve)` | 1 | Multiplies the starting size |
| `with_speed_over_life(Curve)` | 1 | Multiplies the velocity by the curve's change each step, so forces and collisions still act; a curve from 1 to 0 slows a particle to a stop |

A curve and a gradient are keys over a particle's life, `t` from 0 to 1:

```rust
Curve::new([(0.0, 1.0), (1.0, 0.2)]);
Curve::eased([(0.0, 0.0, Ease::Linear), (0.2, 1.0, Ease::OutQuad), (1.0, 0.0, Ease::InQuad)]); // each segment eases by the key it ends at
Curve::constant(1.0);
Gradient::new([(0.0, Color::WHITE), (0.7, Color::YELLOW), (1.0, Color::TRANSPARENT)]);
Gradient::constant(Color::WHITE);
```

A curve that starts at 0 gives a particle no speed of its own for life (the start speed is scaled by it), only what gravity and other forces give it.

### Motion

| Builder | Default | What |
|---------|---------|------|
| `with_gravity(x, y)` | (0, 0) | Pixels per second squared (y is down) |
| `with_drag(f32)` | 0 | Fraction of speed lost per second |
| `with_radial_acceleration(range)` | 0 | Away from the emitter (negative pulls in), one value per particle |
| `with_tangential_acceleration(range)` | 0 | Round the emitter, counter-clockwise on screen when positive, one value per particle |
| `with_noise(NoiseForce)` | none | A smooth, changing force field: `NoiseForce::new(strength, frequency).with_speed(s)`; swirls are about `1 / frequency` pixels across |
| `with_space(Space)` | `World` | `World`: particles stay where they were born as the emitter moves on. `Local`: they move and turn with the emitter |

A `Local` effect's particles live in the emitter's frame, and its forces act there. They ignore `inherit_velocity`, never collide and never feel attractors; a collision on a `Local` effect is refused in a file, and dropped with a warning in code.

### Look

| Builder | Default | What |
|---------|---------|------|
| `with_blend(BlendMode)` | `Alpha` | How particles blend with what is behind them |
| `with_filter(Filter)` | `Default` | Texture filtering |
| `with_lighting(bool)` (`lit`) | true | Whether lights affect the particles |
| `with_emission(f32)` | 1 | Above 1 glows past white and feeds bloom |
| `with_effect(SpriteEffect)` | none | A sprite effect on each particle |
| `with_z(i32)` | 0 | Draw order among sprites |
| `with_draw_order(DrawOrder)` | `NewestOnTop` | Which particle covers which (`OldestOnTop`) |
| `with_frames(Flipbook)` | none | The image as a sprite sheet each particle plays through |
| `with_align(ParticleAlign)` | `None` | `ParticleAlign::Velocity { stretch }` faces where it moves (ignoring rotation and spin), `size + speed × stretch` long |
| `with_layer(bits)` | 0 | The bits attractors' layer masks test (a camera's layers are the entity's `RenderLayers`, below) |
| `with_seed(u64)` | none | Fixes the random stream, so the effect repeats |

<!-- given: spark: Handle<Image>, sheet: Handle<Image> -->
```rust
ParticleEffect::sparks(spark)
  .with_blend(BlendMode::Additive)   // fire, sparks, magic: overlapping particles add up to bright cores
  .with_filter(Filter::Linear)       // smooth when scaled; Filter::Nearest keeps hard texels
  .with_lighting(false)              // full brightness in the dark (glowing embers)
  .with_emission(2.5);               // glows past white, for bloom

ParticleEffect::new(sheet.clone()).with_frames(Flipbook::new(4, 2)); // left to right, then down, once over each particle's life
ParticleEffect::new(sheet)
  .with_frames(Flipbook::new(4, 2).with_count(6).at_fps(12.0).random_start()) // the first 6 frames, 12 a second, looping, each particle from a frame of its own
  .with_align(ParticleAlign::Velocity { stretch: 0.02 })
  .with_effect(SpriteEffect::glow_with_intensity(Color::new(100, 150, 255, 255), 6.0, 1.0));
```

`BlendMode::Alpha` (the default) suits smoke and dust; `Multiply` darkens (soot, shadows), `Screen` lightens softly (a 0..1 formula: over pixels brighter than white it dims them; use `Additive` for light above white).

The prelude calls the particle align type `ParticleAlign` (the UI's `Align` holds that name); a file writes `align: Velocity(stretch: 0.02)`.

A value that is not allowed (not a number, a negative lifetime, a sheet with no cells) never panics: a file with one is refused, naming the field; an effect built in code is clamped, with one warning.

## Effect files

A `*.particles.ron` file under `assets/` holds one effect. Tune it without recompiling: saved while the game runs, it applies at the next step (hot reload is on by default in debug builds with a window; `EngineBuilder::hot_reload`).

<!-- given: x: f32, y: f32 -->
```rust
let sparks = ctx.assets().load_now::<ParticleEffect>("fx/sparks.particles.ron")?;
ctx.world.spawn((Transform::from_xy(x, y), ParticleEmitter::new(sparks)));
```

Every field but `image` may be left out, taking the default in its comment:

```ron
(
  image: "fx/spark.png",
  // Emission
  rate: 40,                             // 10 per second
  rate_per_distance: 0.5,               // 0 per pixel the emitter moves
  bursts: [(time: 0.1, count: (2, 4)), (count: 8, cycles: 3, interval: 0.25)],
  duration: 2.0,                        // 1 s
  looping: true,                        // true
  start_delay: 0.25,                    // 0 s
  prewarm: true,                        // false (needs looping)
  max_particles: 500,                   // 1000
  // Shape
  shape: Circle(radius: 12, arc_deg: 180, thickness: 0.5), // Point
  direction_deg: -90,                   // 0: right, 90: down
  spread_deg: 40,                       // 360
  outward: false,                       // false
  // Spawn values: a number, or a random (min, max)
  lifetime: (0.4, 0.9),                 // (1, 2) s
  speed: (80, 160),                     // (0, 50)
  size: 6,                              // (8, 16)
  rotation_deg: (0, 360),               // 0
  angular_velocity_deg: 30,             // 0 per second
  inherit_velocity: 0.25,               // 0
  color: ("#ffe08a", "#ff7a1a"),        // white; one colour, or a random pick between two
  // Over life: keys in order, each `t` within 0 to 1
  color_over_life: [(0.0, "#ffe08a"), (0.6, "#ff7a1a"), (1.0, "#ff000000")], // white to transparent
  size_over_life: [(0.0, 1.0), (1.0, 0.2, InQuad)], // 1 throughout
  speed_over_life: [(0.0, 1.0), (1.0, 0.5)],        // 1 throughout
  // Motion
  gravity: (0, 300),                    // (0, 0)
  drag: 0.5,                            // 0
  radial_acceleration: (-10, 10),       // 0
  tangential_acceleration: 5,           // 0
  noise: (strength: 40, frequency: 0.02, speed: 2), // none (speed: 1)
  space: World,                         // World or Local
  // Look
  blend: Additive,                      // Alpha, Premultiplied, Additive, Multiply, Screen
  filter: Nearest,                      // Default, Nearest, Linear
  lit: false,                           // true
  emission: 2.5,                        // 1
  effect: Flash(color: (r: 255, g: 0, b: 0, a: 255), intensity: 0.5), // none
  z: 7,                                 // 0
  draw_order: OldestOnTop,              // NewestOnTop
  frames: (columns: 4, rows: 2, count: 6, mode: Fps(12.0), random_start: true), // none
  align: Velocity(stretch: 0.02),       // None
  // Collision and sub-effects
  collision: (radius: 2, mask: 3, response: Bounce(0.6), events: true), // none
  on_death: (effect: "fx/puff.particles.ron", count: 3, inherit_velocity: 0.5), // none
  on_collision: (effect: "fx/puff.particles.ron", count: (2, 4)),               // none
  layer: 2,                             // 0
  seed: 77,                             // none
)
```

- A colour is `"#rrggbb"`, `"#rrggbbaa"` or `(r:, g:, b:, a:)`, its `a` 255 when left out. An ease is an `Ease` name (`InQuad`, `OutCubic`, ...), on the key its segment ends at.
- `Circle` leaves out `arc_deg` for a full circle and `thickness` for a filled one, `Rect` its `thickness` likewise. A burst's `time`, `cycles` and `interval` default to 0, 1 and 0.
- A collision's fields are all optional: `radius` 4, `mask` every layer, `response` `Die`, `Bounce(0.6)` (restitution, 0 to 1), `Stick` or `Event`, and `events` false. An option is written without `Some(..)`.
- `image` and the sub-effects are asset paths, and the effect's dependencies: it is ready once they are, a missing one fails it, and saving one reloads this effect.
- A mistake fails the load: a RON error at its line and column, a bad value naming its field. Unknown fields are refused, so a misspelt one fails rather than doing nothing.
- **Hot reload.** Over-life values (gradient, size and speed curves), forces, the look and collisions change live particles at the next step. Spawn values (lifetime, start size, colour) apply to new particles. An emitter keeps its time and its burst schedule. A `max_particles` lowered below the live count kills nothing: new spawns wait until the count is under it. A reload that drops a `collision` or changes a `Stick` response releases the particles it held, and they move again. A broken save keeps the last good effect, with one warning.
- **The web** has no asset folder and no hot reload: embed the files with a `MemorySource` (`ryo-assets`, section 5; the particles demo's `embedded.rs` is one).

The particles demo's `assets/fx/` has working files; its Files page plays them for you to edit while it runs.

## Emitters

An emitter is an entity with `Transform` and a `ParticleEmitter`. It emits at its `GlobalTransform`, so it can be a child of a moving entity.

<!-- given: x: f32, y: f32, fire: Handle<ParticleEffect>, torch: Entity -->
```rust
ctx.world.spawn((
  Transform::from_xy(x, y),
  ParticleEmitter::new(fire.clone())
    .with_seed(7)       // the same particles each run, over the effect's seed
    .with_z(20)         // its draw order, over the effect's z
    .with_speed(0.5),   // its time as a multiple of game time (0 to 100)
));
ctx.spawn_child(torch, (Transform::from_xy(0.0, -8.0), ParticleEmitter::new(fire)));
```

Other builders: `unscaled()` (real time, below), `stopped()` (emits nothing until `play`), `once()` (one cycle of its effect, even a looping one), `despawn_when_finished()`.

The game controls it through the component, each change done in its next step:

<!-- self: torch: Entity -->
```rust
if let Some(mut emitter) = ctx.world.get_mut::<ParticleEmitter>(self.torch) {
  emitter.stop();         // no new particles; the live ones finish
  emitter.play();         // resumes where it stopped; one whose one cycle ended plays it again from the start
  emitter.burst(20);      // 20 more particles, whatever its effect emits and whether or not it plays
  emitter.clear();        // removes its live particles; it carries on if it plays
  emitter.restart();      // removes them and starts its cycle over at 0, every burst due again, and plays
  emitter.set_speed(0.5);
}
```

It reads back with `is_playing()`, `is_finished()`, `is_unscaled()`, `alive()` (live particles, as the last step left them), `speed()` and `z()`. Setting its public `effect` field to another handle starts the emission over.

An emitter whose effect is still loading (`ctx.assets().load`) emits nothing and warns nothing, and starts its first cycle in the step its effect lands.

### One-shots

<!-- given: x: f32, y: f32 -->
<!-- self: boom: Handle<ParticleEffect> -->
```rust
// One cycle of the effect, then its emitter despawns itself
ctx.particles().play_at(&self.boom, Vec2::new(x, y));

// The same, spelled out
ctx.world.spawn((
  Transform::from_xy(x, y),
  ParticleEmitter::new(self.boom.clone()).once().despawn_when_finished(),
));
```

`play_at` returns the entity (move it, parent it or despawn it as any other). `ctx.particles()` also has `count()` (live particles over every emitter), `budget()` and `clear()` (removes every live particle at once; the emitters carry on). A system takes the `Particles` parameter instead, with `play_at` (through the system's commands), `count` and `budget`:

<!-- with: #[derive(Resource)] struct Sparks(Handle<ParticleEffect>); #[derive(Component)] struct Hit; -->
```rust
fn sparks_on_hits(
  mut commands: Commands,
  mut particles: Particles,
  sparks: Res<Sparks>,
  hits: Query<(Entity, &Transform), With<Hit>>,
) {
  for (entity, transform) in &hits {
    particles.play_at(&sparks.0, transform.translation);
    commands.entity(entity).remove::<Hit>();
  }
}
```

### Finishing

An emitter is finished when it is not playing (stopped, or its one cycle over) with no live particles and no burst still to add. Nothing happens to a finished emitter by itself: without `despawn_when_finished` it stays until the game despawns it.

- `EmitterFinished { entity }` is sent once per finish, in the step the emitter finishes. `play` and `restart` arm it again.
- `despawn_when_finished` despawns the entity, with its children, in the next step. A game-time emitter's entity is therefore still there for a scene's next `update`. An unscaled emitter can do both within one frame, when the frame runs several of its steps, so its entity may be gone when the message is read.
- `burst` on a `stopped()` emitter adds particles, but such an emitter never reports `EmitterFinished` and never despawns: only `play` or `restart` arm it. To fire and forget, use `play_at`, or `once().despawn_when_finished()`.

<!-- self: fuse: Entity -->
```rust
for finished in ctx.messages::<EmitterFinished>() {
  if finished.entity == self.fuse {
    // Its last particle has died
  }
}
```

**Despawning an emitter removes its particles.** So does taking the `ParticleEmitter` off its entity, and a scene `Switch` or `Reset`, which despawn the scene's entities (a `Push` and `Pop` keep them). To let live particles finish after the source goes, give the emitter `despawn_when_finished()` when you spawn it and call `stop()` on it later.

### Teleports

Moving an emitter far in one step would spread its stream along the jump. Call `ctx.skip_interpolation(entity)` after the move, on any emitter, `Interpolated` or not: its next step spawns at the new place only, so a teleport never streaks. An emitter just spawned or just enabled again does not streak either.

<!-- given: to: Vec2 -->
<!-- self: portal: Entity -->
```rust
if let Some(mut transform) = ctx.world.get_mut::<Transform>(self.portal) {
  transform.translation = to;
}
ctx.skip_interpolation(self.portal);
```

### Disabled emitters

A `Disabled` emitter is frozen and hidden: its particles neither move, age nor draw until it is enabled again. A stopped emitter's live particles keep full motion and collisions.

## What the engine does

- **The step.** In each fixed step, after `Scene::update`, physics and transform propagation (`FixedSet::Particles`), every emitter spawns, moves and removes its particles. An emitter spawned in `update` emits that step, from its placed, lifted point.
- **The draw.** Each frame every emitter with live particles that a camera sees joins the draw list as one run of sprites, each particle blended between its last two steps, so particles are smooth at any refresh rate.
- **Layers and order.** An emitter is drawn by the cameras that share its entity's `RenderLayers` (layer 0 by default). It sorts by its z (`with_z` on the emitter, else the effect's) with entities, sprites and trails; at equal z it draws after entities, world text and `Scene::render` sprites and before parallax, trails and lines, emitters among themselves in entity order. Inside one emitter the newest particle is on top unless the effect says otherwise, and the order never changes from frame to frame.
- **Culling.** An emitter whose box no camera sees is not drawn, but it keeps simulating, so nothing pops when it comes back into view. Particles of a visible emitter are not culled one by one.
- **Game time.** Particles run on game time: a paused clock and hitstop freeze them (no step runs), and slow motion slows them, with the same particles at any frame rate.
- **They freeze with their scene.** An emitter belongs to the scene whose code or systems added it, and a game-time emitter steps while that scene updates, so not under a scene that stops it (a pause menu pushed over the game holds the game's particles while the menu's own keep moving). A scene transition doesn't stop it: particles keep moving through a fade. An emitter added outside every scene (by a game-wide system), or one whose scene has gone, is held while any scene is held.
- **`unscaled()`** runs an emitter on real time through all of that, for menu effects: it steps once a frame in fixed steps of the game's step length, at most 8 a frame (a longer frame's time beyond them is dropped).
- **The budget.** At most `DEFAULT_PARTICLE_BUDGET` (20,000) particles live at once, over every emitter; `EngineBuilder::particle_budget(n)` sets another. It is a safety cap, not a quality setting: a spawn past it is dropped, with one warning naming the effect, and live particles are never cut short. Each emitter also keeps its effect's `max_particles`. F1 shows `Particles: live / budget`.
- **Seeds.** Each emitter has its own random stream, seeded by `with_seed`, else the effect's `seed`, else the engine (fixed in a headless run, so a headless scene repeats without seeding). Adding or removing another emitter, entity or resource never changes a seeded emitter's particles.
- **Cost.** A CPU simulation on the main thread, drawn through the sprite batch. On an Apple M5 Pro 10,000 particles from one emitter cost about 0.4 ms of a frame, and 2,000 colliding over a 64 x 64 tile map about 0.2 ms; a steady frame allocates nothing. The profiler's scopes are `cpu:particles` and `render:particles`.

## Sub-emitters

A particle can play another effect's burst where it dies or meets a collider, for splashes, fireworks and impacts:

<!-- given: sprite: Handle<Image>, splash: Handle<ParticleEffect> -->
```rust
ParticleEffect::new(sprite)
  .with_collision(CollisionConfig::die_on_hit(2.0))
  .on_collision(SubEmitter::new(splash.clone(), (2.0, 4.0)).with_inherit_velocity(0.2)) // count, then a share of the particle's velocity
  .on_death(SubEmitter::new(splash, 1.0));
```

- `on_death` plays where a particle's life ends or a `Die` collision removes it. `on_collision` plays on any contact that would send a `ParticleCollision`, whether or not events are on; a `Die` contact plays both.
- A sub-effect's shape, direction and start values apply where the particle was. Its emission (rate, bursts, cycle, prewarm), its `seed` and its own `inherit_velocity` play no part.
- Its particles live in the emitter's pool, move, collide and look as the sub-effect says, draw at the emitter's z just over its own particles, count against the sub-effect's `max_particles` and the budget, and go with the emitter.
- Sub-effects go one level deep: a sub-effect's own `on_death` and `on_collision` are ignored, with a warning. A sub-effect still loading spawns nothing, with no warning.

## Attractors

An attractor is an entity with `Transform` + `ParticleAttractor`; forces from several attractors add up. They act on `World` effects only.

<!-- given: x: f32, y: f32 -->
```rust
ctx.world.spawn((Transform::from_xy(x, y), ParticleAttractor::point(200.0, 100.0))); // strength, radius; negative repels
ParticleAttractor::gravity_well(300.0, 150.0);
ParticleAttractor::vortex(150.0, 80.0);
ParticleAttractor::spiral(150.0, 80.0, 40.0); // strength, radius, inward pull
ParticleAttractor::line(-100.0, 0.0, 100.0, 0.0, 120.0, 60.0);
ParticleAttractor::area(800.0, 600.0, 50.0, 0.0); // width, height, force x, force y
ParticleAttractor::turbulence(30.0, 50.0, 1.0) // strength, scale, speed
```

`with_layer_mask(mask)` limits an attractor to particles whose effect has a shared bit in `with_layer(bits)`. Effects default to layer 0, which only unmasked attractors affect. `with_enabled(false)` switches one off.

Following the mouse:

<!-- self: attractor: Entity -->
```rust
let (mx, my) = ctx.input.mouse_position();
let target = ctx.camera().screen_to_world(Vec2::new(mx, my));
if let Some(mut transform) = ctx.world.get_mut::<Transform>(self.attractor) {
  transform.translation = target;
}
```

## Collisions

`with_collision(CollisionConfig)` makes an effect's particles collide with entities that have `Collider` (no `RigidBody` needed); the examples build the configs it takes. Geometry is the collider as physics places it: a circle is a circle; a segment, polyline or segments collider (a tile map's colliders among them) is its edges, which a particle meets within its radius, or by crossing one in a step, and bounces off to the side it came from; every other shape is its world bounds, a box (`Collider::world_bounds`). A tile map's ground has one side, the outside: a particle that starts in it (landing dust spawned round the feet) goes out through the nearest face, so it ends on top of the ground. It is judged from the faces near it, up to half a cell away: a particle deep inside a thick mass stays there, and one in a thin floor leaves through the nearer face, the bottom if it is nearer, so spawn colliding particles above the surface, or within half the floor's thickness of it. Every other edge, a game's own polyline or segment, closed or not, is hollow as physics treats it: a particle meets it from either side and stays on the side it came from (particles stay inside a polyline arena), so spawn colliding particles on the side they belong. Trigger colliders, `Disabled` entities and the emitter's own collider are skipped. Each particle is a circle of the config's radius.

```rust
CollisionConfig::bouncy(2.0, 0.7); // radius, restitution
CollisionConfig::sticky(2.0); // frozen until its lifetime ends
CollisionConfig::die_on_hit(2.0).with_events();
CollisionConfig::new(2.0)
  .with_mask(0b10) // matched against Collider::layer
  .with_response(CollisionResponse::Event);
```

| Response | Behaviour | `ParticleCollision` messages |
|----------|-----------|------------------------------|
| `Die` | Particle removed | Once, with `with_events()` |
| `CollisionResponse::bounce(r)` | Reflects with restitution | With `with_events()`, on contact entry at an impact speed of at least `MIN_BOUNCE_EVENT_SPEED` (20 px/s) |
| `Stick` | Stops on the surface, its radius out (where it crossed an edge, for a fast one); still ages and fades | Once, with `with_events()` |
| `Event` | Passes through | Always, on contact entry |

Contact entry is tracked per particle, not per collider: sliding from a floor into a wall does not notify again. Collisions are checked at particle positions once per step, so very fast particles can pass through thin boxes and circles; an edge also stops a particle whose move crossed it.

**On a storey of a tile map** (an emitter with a `Floor`, or held by an entity with one, `ryo-tilemap`), particles start where the emitter draws, its storey's height up, including the burst of an emitter spawned and fired in one step, but colliders and attractors stay where their entities stand: a lifted emitter's sparks pass through the storey's drawn surface and meet the colliders a storey's height below. A storey's edge, which keeps characters on a raised floor, is no surface: particles pass it.

`ParticleCollision` is a message, written in the particle step, so a scene's `update` reads it on the next step:

```rust
for hit in ctx.messages::<ParticleCollision>() {
  // hit.emitter_entity, hit.collider_entity
  // hit.position_x / position_y: the particle at contact
  // hit.normal_x / normal_y: the surface normal, pointing at the particle
  // hit.velocity_x / velocity_y: the velocity before the response
}
```

## Tips

- Try a preset before building a custom effect.
- Keep `max_particles` in the hundreds per emitter.
- Drag shortens how far particles travel without changing their lifetime.
- Collisions go through a grid of the colliders, so a particle meets only those near its move: cost follows the colliders near each particle, not how many the level has. A collider far from the rest coarsens the grid and costs more, with the same results.
- A prewarm is paid at once: about 0.43 ms for 1,000 particles over a 2 s cycle, in the step it runs.
- Glow and outline sprite effects draw past the particle's quad and are not counted in the emitter's cull box, so their edges can be cut at the view's edge.
- The particles demo (`examples/demos/particles`) has a page for each of the above.
