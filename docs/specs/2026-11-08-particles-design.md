# Particles Design

## Overview

**Intent.** Particles should work the way they do in a professional engine. An effect is a definition, written in a file or built in code. A game places it on an entity, and the engine spawns, simulates, collides and draws its particles every step. A game never calls an update or a render function for particles. Effects carry the standard tools for shaping a look over a particle's life, reload while the game runs, freeze when the game pauses, and stay cheap: thousands of particles cost well under a millisecond, and a runaway effect cannot take the frame down. (Owner, 2026-11-08: "skip" the questions, design it as a professional engine would, as was done for saves.)

**Why now.** This is the second of the six sub-projects left from the engine audit (`docs/research/2026-10-11-engine-audit.md`, "Particles": "keep the design, fix the hot path, let the engine drive it, add a global cap, fix the docs"). Checked against the code on 2026-11-08 (main 74fd1521):

- **Each scene drives its own particles.** `ParticleSystem` is a plain struct a scene owns. The scene calls `update(world, dt)` in `Scene::update` and `collect_for_render(world, batch)` in `Scene::render`. Leap-king builds four of them and calls `update` from three places.
- **The hot path allocates and hashes.** Every update builds four or five `QueryState`s, two `Vec`s with cloned `CollisionConfig`s and attractors, and two `HashMap`s. Each particle pays two `HashMap` lookups a frame, one in the update (`particles/mod.rs:727`) and one in the draw (`:1120`). Attractors and colliders are tested by brute force against every particle.
- **No overall limit.** Each emitter has a `max_particles` (checked against a count one update stale), and nothing bounds the total.
- **The draw path is limited.** Particles go through the scene's `BatchCollector`, so they ignore `RenderLayers` (every camera on layer 0 draws them) and are never interpolated between fixed steps. They are not culled before submission. Particles are removed with `swap_remove`, so overlapping particles change draw order from frame to frame.
- **Looks are two-point.** Size and colour go from a start value to an end value in a straight line. There are no curves, gradients, sprite-sheet frames, sub-emitters, local space or emission over distance. `Particle::frame`, `mass` and `has_trail` exist but nothing uses them.
- **Determinism depends on query order.** One `StdRng` serves every emitter in query (archetype) order, so adding an unrelated component can change a seeded run. Headless runs seed audio but not particles.
- **Pausing works by accident.** Particles freeze under a pause menu only because the game scene's `update` is not called.
- **Docs are wrong.** The module is titled "GPU Particle System" (`mod.rs:1`; `README.md:149` says "GPU-batched") and claims "10k+ particles at 60fps", which nothing measures. Trails are promised and don't exist. There is no particle bench and no allocation test.

The audit's string-hash texture lookups and `gen_range` panics are already fixed.

**Who it serves.** First, the player: smooth effects at any refresh rate, frozen when the game pauses, and no frame spikes from a burst. Second, the game developer, often an agent: one line to place or play an effect, and a file to tune it without recompiling.

## Goals

- **The engine drives particles.** An entity with a `ParticleEmitter` emits. The engine spawns and simulates in its fixed step, after transforms are propagated and floors lifted. It draws in the frame's draw list, interpolated, culled, and on the emitter's render layers. `ParticleSystem` goes away.
- **Effects are assets.** A `ParticleEffect` is loaded from a `*.particles.ron` file through the asset registry, or built in code and added. Many emitters share one effect. A saved file applies on the next step, and a broken save keeps the last good effect and warns once.
- **One-line playback.** `ctx.particles().play_at(&effect, position)` plays an effect once and cleans up after it. Emitters can play, stop (live particles finish), clear, restart and burst. An emitter can despawn itself when its last particle dies, and `EmitterFinished` reports the end.
- **Shaped over life.** Colour follows a multi-stop gradient and size and speed follow curves over each particle's life. Start values are a number or a random `(min, max)` range, and start colours a random pick between two.
- **Standard emission.** Rate over time, rate over distance moved, timed bursts with repeats, a duration with looping, a start delay and prewarm. Shapes: point, circle or arc (filled to edge-only), rectangle (filled to outline) and line, with a direction and spread. Particles can inherit part of the emitter's velocity, and can live in world space or in the emitter's own space.
- **Standard motion.** Gravity, drag, radial and tangential acceleration, smooth seeded noise, the existing attractors, and the existing collisions.
- **Standard looks.** Sprite-sheet frames (over life or at a frame rate, optionally from a random frame), alignment to velocity with stretch, blend, filter, lit or unlit, emission (glow), built-in sprite effects, z order, and a stable draw order (newest on top by default).
- **Sub-emitters.** A particle's death or collision can play another effect's burst where it happened, for splashes, fireworks and impacts.
- **Pause and time.** Particles use game time: a paused game clock, hitstop and slow motion apply, and a scene that is not updated (one under a pause menu) has its particles held. An emitter marked unscaled runs on real time through all of them, for menu effects. Each emitter has its own speed scale.
- **A global budget.** One engine-wide particle budget, set on the builder. A spawn past it is dropped, with one warning naming the effect. The F1 stats show particles against the budget.
- **Fast.** No per-particle hash lookup and no per-frame allocation in steady state. The targets below are measured against today's system.
- **Deterministic.** Each emitter has its own random stream, seeded from the emitter, the effect, or the engine (fixed in headless runs). Adding or removing one emitter never changes another's particles. The same steps give the same particles, whatever the frame rate.
- **Nothing lost.** Today's collisions (every response, the events, the tile-edge rules, a storey's edge not counting as a surface), attractors with their layer masks, floors, the presets and the never-panicking ranges all carry over, with their tests.
- **Honest docs.** The CPU simulation and sprite-batch drawing are described as they are, and the skills, module notes and overview follow the new API.

## Non-goals

These may become later work:

- GPU simulation. WebGL2 has no compute shaders, and the web is a target. The CPU path runs everywhere, and the budget and targets cover 2D needs.
- Per-particle trails or ribbons. The trail module serves entities, and per-particle ribbons are later work.
- A visual effect editor. Files with hot reload are the authoring loop.
- Particles pushing physics bodies, or replicated over the network.
- Screen-space or UI particles.
- Mesh particles, and lights carried by particles.
- Multithreaded simulation. It is a later lever if a game needs more than the budget allows.
- Particles colliding per storey. They collide in ground space, as today (the recorded floors limit).
- Moving bullet-hell's, the cursor demo's and the bloom demo's hand-made particles onto the engine's system.

## Anti-goals (the professional bar)

Any of these would be a failure even if everything else works:

- a game needing to call anything every frame for its particles to move or draw;
- a steady particle frame allocating, or hashing per particle;
- a mistake in an effect file crashing the game or blanking a working effect;
- particles moving while the game is paused or their scene is held under a pause menu, unless the emitter asked to run unscaled;
- the same seed and steps giving different particles at a different frame rate, or after an unrelated entity or resource is added;
- one runaway emitter growing the particle count without bound;
- overlapping particles swapping draw order from frame to frame;
- a moving emitter's continuous stream clumping at step boundaries instead of spreading along its path;
- losing any of today's collision, attractor or floor behaviour;
- a panic from a value a game or file can set (NaN, a negative lifetime, an empty range, a zero-cell sprite sheet).

## Decisions

### The owner's decisions

- Design particles as a professional engine would, without questions (2026-11-08, "skip").

### My calls (the owner may revisit them in review)

- **Definition and instance are separate.** `ParticleEffect` is the asset (what the effect looks like and does), and `ParticleEmitter` is the component (one placement: its playing state, its time, its random stream). This is how Unity, Godot and Bevy Hanabi split it, and it is what makes files, sharing and hot reload work.
- **Particles step in the fixed step** and draw interpolated, as entities do. The engine's simulation is fixed-step throughout, so particles collide with colliders where physics put them, stay frame-rate independent and deterministic, and freeze for free when no step runs. Each particle keeps its previous position, and the draw blends by the frame's step fraction.
- **Unscaled emitters** step on real time in a per-frame system, with their own accumulator at the same fixed step length. They also run under a pause overlay and a paused clock.
- **Particles freeze with their scene.** A game-time emitter belongs to the scene whose code or systems added it, and steps only while that scene updates. This is the rule audio already uses for its emitters (`audio::in_scene`, `hold_scenes`): an emitter added outside every scene, or one whose scene has gone, is held while any scene is held. So a pause menu over the game freezes the game's particles, and the menu's own particles keep moving. Today the same thing happens only because each scene drives its own system. Engine animation and tweens keep running under an overlay; particles follow audio and physics instead.
- **Particles vanish with their emitter.** Despawning an emitter entity removes its particles, as Unity and Godot do by default. To let particles finish after the source is gone, stop the emitter and set it to despawn when finished. This also clears particles on a scene `Switch` and `Reset`, which despawn the scene's entities.
- **A `Disabled` emitter is frozen and hidden.** Its particles neither move nor draw until it is enabled again. Today they keep flying without gravity or collisions and don't draw. A stopped emitter's live particles keep full motion and collisions; today an `enabled = false` emitter's particles lose gravity.
- **The budget defaults to 20,000 particles.** It is a safety cap, not a quality setting: at the target cost it bounds the particle step to a few milliseconds. When it is full, new spawns are dropped and live particles are never killed early. The emitter's own `max_particles` stays, checked against its live count.
- **Angles in effects are in degrees, and their names say so** (`direction_deg`, `spread_deg`, `rotation_deg`, `angular_velocity_deg`), in files and in code alike. Effects are authored data, and Godot and Unity author them in degrees. The rest of the engine keeps radians, so the suffix removes any doubt.
- **Ranges.** A spawn value is a number or a `(min, max)` pair, sampled uniformly. A min above its max samples the min, as today.
- **Curves and gradients.** A curve is a list of `(t, value)` keys, eased per segment with the engine's `Ease` (which gains serde). A gradient is a list of `(t, colour)` keys, mixed linearly as stored, like today's start-to-end mix. Both are sampled into a small table when the effect is built or loaded, so a particle reads them at constant cost.
- **Colours in files** can be written `"#rrggbb"`, `"#rrggbbaa"` or `(r:, g:, b:, a:)`, as UI files take them.
- **Over-life values follow a reload.** A saved effect changes the curves, gradients, forces, look and collisions of live particles at once. Spawn values (lifetime, start size, start colour) apply to new particles. An emitter keeps its time and its burst schedule across a reload.
- **Sub-emitters go one level deep.** A sub-effect's particles live in the parent emitter's pools, draw at its z and vanish with it. A sub-effect that has sub-emitters of its own loads with a warning, and those are ignored.
- **Noise** is a small seeded 2D gradient noise in the core, used for the noise force and the turbulence attractor. The core cannot depend on the optional procgen crate's noise.
- **Draw order inside an emitter** is spawn order, newest on top by default; an effect can choose oldest on top. Dead particles are removed by compacting the pool in order during the step, never by `swap_remove`.
- **Emitters draw in z order, then entity order**, as particles today sort with entities, sprites and trails (at equal z: after entities and text, before trails and lines).
- **An emitter draws as one run.** Each emitter (and each of its sub-effect pools) is one draw-list item holding its particles' instances in draw order, sorted once and culled once per camera by the emitter's bounds, with the instances written straight into the batch's instance stream. One draw-list item per particle, as entity sprites have, would spend most of the frame target on sorting and culling (10,000 entity sprites cost about 820 µs over an empty frame today, `docs/backlog/assets.md`). A run is how particle systems are drawn in Unity and Godot.
- **Culling.** Each emitter keeps its particles' bounds from the step. An emitter outside a camera is not drawn by it; particles of a visible emitter are not culled one by one (the GPU clips them). Simulation always runs, so nothing pops when an emitter comes back into view.
- **Targets measure what the player feels:** the whole frame over an empty frame, the way tile maps and assets are measured, with the particle systems' own scopes recorded beside them. Only the no-emitters target, too small to see in a frame, is timed on the systems themselves.
- **The allocation guard covers one-shots.** Its measured frames include steady emitters, and also effects played with `play_at` that despawn when finished, the commonest real use. Pools come back through a free list, so the particle code allocates nothing. Allocation sites inside the ECS's own spawn and despawn code are printed and not asserted, as the GPU's are today.
- **One branch, one merge,** as audio and saves went in. The plan builds the engine-driven core and moves the demo and leap-king onto it first, then adds the features, files, demo pages and docs.
- **Seeds.** An emitter's seed is, first, `ParticleEmitter::with_seed`, then the effect's `seed`, then one drawn from the engine's particle stream when the emitter is added (fixed in headless runs, as audio is; from entropy otherwise).
- **`ctx.particles()` and the `Particles` system parameter** follow the saves and audio pattern: a view over the world in scenes, a twin for systems, and tolerance of a world without the particle resources.
- **Presets** stay, as `ParticleEffect` constructors (`ParticleEffect::fire(image)` and the other fifteen) with the same looks.
- **The demo and leap-king** move to the new API. The demo keeps its ten themed pages and gains pages for what is new.

## Design

### Effects

A `ParticleEffect` is an asset with public fields, a `Default`, and `with_*` builders, like `Sound`. Its parts:

- **Emission:** `rate` (per second), `rate_per_distance` (per world unit the emitter moves), `bursts` (each with a time, a count range, a number of cycles, where 0 repeats forever, and an interval), `duration` (seconds), `looping`, `start_delay`, `prewarm`, `max_particles`.
- **Shape:** `Point`, `Circle { radius, arc_deg, thickness }`, `Rect { size, thickness }` and `Line { length }`. A thickness of 0 emits from the edge and 1 from the whole area. The shape has a `direction_deg` and `spread_deg`, or emits outward from the shape. It turns and scales with the emitter.
- **Spawn values:** `lifetime`, `speed`, `size`, `rotation_deg`, `angular_velocity_deg`, `color` (a colour or a random pick between two), and `inherit_velocity` (the share of the emitter's velocity a particle starts with).
- **Over life:** `color_over_life` (a gradient that multiplies the start colour), `size_over_life` and `speed_over_life` (curves that multiply the start values).
- **Motion:** `gravity`, `drag`, `radial_acceleration`, `tangential_acceleration` (about the emitter), `noise` (strength, frequency, scroll speed), and `space` (`World` or `Local`).
- **Look:** `image`, `frames` (a sprite-sheet grid of columns, rows and frame count, played over life or at a frame rate, from frame 0 or a random one), `align` (none, or along velocity with a stretch by speed), `blend`, `filter`, `lit`, `emission`, `effect` (a built-in sprite effect), `z`, and `draw_order`.
- **Collision:** `collision` (today's `CollisionConfig`), and `layer` (the bits attractors' masks test, as today).
- **Sub-emitters:** `on_death` and `on_collision`, each an effect with a count range and an inherited share of the particle's velocity.
- **`seed`** (optional).

An effect is checked when it is built or loaded. Non-finite numbers, negative lifetimes, sizes or rates, a sprite-sheet grid with no cells, and curve keys outside 0..1 or out of order are refused when loading a file, with a message naming the field. Code-built effects are clamped the same way, with one warning.

### Effect files

A `*.particles.ron` file holds one effect. It mirrors the code fields with defaults for everything, as `.sound.ron` does: plain serde with unknown fields refused, a check step, then the runtime effect. Images and sub-effects are asset paths, loaded as dependencies, so a change to the image or a sub-effect reloads the effect too. RON errors carry line and column.

```ron
(
  image: "fx/spark.png",
  rate: 40,
  lifetime: (0.4, 0.9),
  speed: (80, 160),
  direction_deg: -90,
  spread_deg: 40,
  gravity: (0, 300),
  color_over_life: [(0.0, "#ffe08a"), (0.6, "#ff7a1a"), (1.0, "#ff000000")],
  size_over_life: [(0.0, 1.0), (1.0, 0.2, InQuad)],
  blend: Additive,
  collision: (radius: 2, response: Bounce(0.6)),
  on_collision: (effect: "fx/spark_hit.particles.ron", count: (2, 4)),
)
```

(The schema is the planner's to settle in detail. This example shows its spirit.)

### Emitters

`ParticleEmitter::new(effect)` takes a `Handle<ParticleEffect>`. It has `with_seed`, `with_z` (overrides the effect's z), `with_speed`, `unscaled`, `stopped` (starts without playing) and `despawn_when_finished`. At run time it has `play`, `stop`, `clear`, `restart`, `burst(n)` (extra particles now, in any emission mode), `is_playing`, `is_finished` and `alive`.

An emitter takes its render layers from the entity's `RenderLayers`, and its position from the entity's `GlobalTransform` plus its floor lift. A `Local` effect's particles are drawn through the emitter's current transform.

A continuous stream from a moving emitter spreads its spawns along the step's movement, each aged by the part of the step that is left, so a fast emitter leaves an even line.

### The engine's part

- **State.** A crate-internal resource holds one pool per emitter, found through a crate-internal component on the emitter entity, never through a per-particle lookup. A pool keeps its particles' fields side by side in arrays, in spawn order. It owns its sub-effect pools, its random stream and its bounds. Component hooks free a pool when its emitter is removed or despawned.
- **Step.** A new fixed-step set runs after transforms are propagated and floors lifted, so an emitter spawned and fired in one step fires from its placed, lifted point (today's "fires low" limit goes). In order, it: reads effect versions and refreshes changed emitters; gathers attractors and, only when some emitter collides, colliders, into reused buffers; spawns, steps, collides and removes particles emitter by emitter, in entity order; plays sub-emitter bursts; then writes `ParticleCollision` and `EmitterFinished` and despawns finished emitters that asked to be. A per-frame system steps unscaled emitters on real time.
- **Collisions.** Today's contact code and tile-edge rules carry over unchanged. A per-step grid of the gathered colliders and edges replaces testing every particle against every collider, so cost follows the colliders near each particle.
- **Draw.** An engine-owned particle drawer, with its query state kept between frames, pushes one run per visible pool into the frame's draw list, with the emitter's z, layers, bounds, blend, filter, lit flag and emission. The draw list gains a run item kind that sorts as `Sprite` does and expands into consecutive instances when the batch is prepared. Each particle sits at its position blended by the step fraction, and at its sheet frame's UV rectangle.
- **Budget.** The resource counts live particles against `EngineBuilder::particle_budget(n)` (default 20,000), warns once per effect when a spawn is dropped, and feeds a "Particles" row in the F1 stats.
- **Messages.** `ParticleCollision` is unchanged and is written in the particle step, so a scene's `update` sees it on the next step. `EmitterFinished { entity }` is new.
- **Profiling.** `cpu:particles` and `render:particles` scopes.

### Errors

- A file that does not parse or check fails to load, or on a reload keeps the last good effect, warned once with the registry's usual message.
- A missing image or sub-effect fails the effect as a dependency error naming both paths.
- A spawn past the budget is dropped, warned once per effect.
- No value a game or file sets can panic the step.

## Public API and migration

Breaking changes are fine (owner's standing rule).

- **Removed:** `ParticleSystem`, the public `Particle`, `EmissionMode`, the old `EmissionShape`, and `ParticleEmitter`'s look and emission fields and builders (they move to the effect).
- **Added:** `ParticleEffect`, the curve, gradient and range types, `ParticlesView` (`ctx.particles()`), `Particles`, `EmitterFinished`, and `EngineBuilder::particle_budget`.
- **Kept:** `ParticleAttractor`, `AttractorType`, `CollisionConfig`, `CollisionResponse`, `ParticleCollision`, `MIN_BOUNCE_EVENT_SPEED`, and the presets (now on `ParticleEffect`).
- **Users:** the particles demo and leap-king (both scenes) move over. No other game or demo uses the module.

## Acceptance

### Correctness (tests)

- **Engine-driven.** An emitter spawned with no scene code emits and draws. A scene `Switch` leaves no particles behind. A `Push` and `Pop` keep them.
- **Time.** A paused clock, hitstop and a pause menu over the game freeze the game's emitters, while the pause menu's own emitters move; an unscaled emitter keeps going through all three. Speed 0.5 halves the distance travelled. 30, 60 and 144 Hz frame rates give identical particles after the same steps.
- **Determinism.** The same seed gives the same particles. Adding an unrelated emitter, entity or resource leaves a seeded emitter's particles unchanged. Headless runs are repeatable without seeds.
- **Lifecycle.** Despawning an emitter removes its particles. `stop` lets live particles finish. `despawn_when_finished` despawns after the last particle dies and sends `EmitterFinished`. `play_at` cleans up after one cycle of a looping effect.
- **Emission.** Rate, rate over distance (a known distance gives the expected count), bursts with cycles and interval, duration and looping, start delay, prewarm (the first frame looks steady), and `max_particles`.
- **Shapes and space.** Each shape's spawn points and directions stay inside their bounds. Local particles follow a moving emitter; world particles don't. Inherited velocity is applied.
- **Over life.** Gradient and curve sampling at keys and between them, each `Ease` on a segment, and sprite-sheet frame selection in both modes.
- **Motion.** Gravity, drag, radial and tangential acceleration, and noise behave as specified. Attractors keep their tests.
- **Collisions.** Every existing collision test passes against the new storage. Sub-emitters fire on death and on collision, one level deep.
- **Budget.** Spawns stop at the budget and resume as particles die, with one warning.
- **Draw.** Render layers are respected, draws are interpolated between steps, an off-screen emitter submits nothing yet keeps simulating, draw order stays stable as particles die, and an emitter's run sorts against entities, sprites and trails by z as particles do today.
- **Files.** A sample file loads with every field. A broken reload keeps the last good effect. Unknown fields and bad values are refused, naming the field. A saved file changes live particles' over-life values on the next step.
- **Safety.** NaN, infinite, negative and reversed values from code and files never panic.
- **Floors.** An emitter on a storey emits at its lifted point, including in the step it was spawned.

### Targets (release, record machine, quiet, `uptime` recorded)

The plan's first task measures today's system on the same benches, so every target has a before figure.

Frame targets are the whole frame over an empty frame (one fixed step a frame, the GPU drained outside the measure), as the tile map and asset benches measure.

- **10,000 particles** (one emitter, no collisions or attractors): ≤ 1.0 ms a frame.
- **100 emitters of 100 particles each:** ≤ 1.2 ms a frame.
- **2,000 colliding particles** over a 64×64 tile map's colliders: ≤ 1.0 ms a frame.
- **No engine-own allocation** in a steady particle frame, by the `tests/*_alloc.rs` method: steady emitters spawning and losing particles, plus `play_at` one-shots despawning when finished during the measured frames. ECS spawn and despawn sites are printed, not asserted.
- **No emitters:** the particle systems' own time ≤ 1 µs a frame.
- **Recorded beside each frame target:** the `cpu:particles` and `render:particles` scopes.
- **Recorded, with no target:** prewarm cost and an effect file's reload time.

### Demo

`examples/demos/particles` keeps its ten themed pages (fire, smoke, explosions, magic, weather, impacts, celebration, trails, attractors, combined), on the new API. New pages:

- **Life:** gradients, size and speed curves, and sprite-sheet frames.
- **Emission:** bursts, rate over distance behind the mouse, the shapes, and local against world space.
- **Collisions:** rain splashing on a tile map, sparks bouncing, embers sticking, all with sub-emitters.
- **Files:** effects loaded from `assets/fx/*.particles.ron`, with a note to edit and save one while the demo runs.

The demo tour is repeatable run to run, because headless runs seed particles.

## Documentation

- **`ryo-particles`:** rewritten around effects, files, emitters and the engine's part.
- **Other skills:** `ryo-api`, `ryo-assets` (the new asset type), `ryo-game-dev`, `ryo-renderer`, `ryo-messages`, `ryo-lighting` and `ryo-tilemap` (floors), each bumped by a minor version, once.
- **Module notes** rewritten. `testing/AGENTS.md` loses the seeding advice. The overview, `README.md` and `examples/demos/AGENTS.md` are corrected.
- **Records:** `docs/backlog/particles.md` holds the measured figures and limits.

## Risks

- **Scope.** This is a large feature set. The plan builds the engine-driven core and the migration first, so the engine works on the new path before the features land.
- **Collision regressions** from the new storage and the grid. The existing tests are ported first, before anything changes, and pass at every task.
- **Tour pixels** change on the particles pages by design (new seeding and interpolation). Every other page must match main.
- **A busy record machine.** Measurements follow the quiet-machine rule and record `uptime`.

## Details settled at planning (2026-11-08, my calls)

The code maps made at planning settled these. The owner may revisit any of them.

- **Order of the work.** Today's system moves aside into a `legacy` module while the new one is built beside it. The demo and leap-king move over once the engine-driven core draws, and then the old system is removed.
- **The random stream and the noise are the engine's own**: SplitMix64 for the stream and a 2D gradient noise. A `rand` upgrade then can't change a seeded effect.
- **Effect defaults:**
  - rate 10 a second, a 1 s looping cycle, 1000 particles at most;
  - any direction (`spread_deg` 360), lifetime 1 to 2 s, speed 0 to 50, size 8 to 16;
  - white, fading to transparent over life.
- **The emitter's scale moves the shape only.** Particle size and speed are not scaled. A mirrored scale mirrors directions, and a zero scale emits from the centre.
- **Speed over life** scales each particle's velocity by the curve's change each step, so forces and collisions still act. A curve from 1 to 0 slows particles to a stop.
- **Stretch along velocity:** a particle moving at speed v is drawn `size + v × stretch` long.
- **`Local` effects don't collide or feel attractors.** Setting a collision on one is reported and dropped.
- **Prewarm** applies to looping effects only, and runs at most 10 s of steps.
- **A teleport doesn't streak.** An emitter spreads its stream along its move unless `skip_interpolation` was called for it, or it was not stepped in the previous step (just spawned or re-enabled).
- **Sub-emitter triggers:** `on_death` includes a `Die` collision, and `on_collision` fires on any contact that would send an event, whether or not events are on.
- **Per-particle radial and tangential values** come from each particle's own seed, so each keeps one value without extra storage.
- **Unscaled emitters** take at most 8 steps a frame and drop the rest of the time.
- **The collision grid's cell** is twice the mean edge length, clamped to 16 to 256 world units. Results must equal testing every edge.
- **Presets** keep their values. A line's old offset from the emitter is dropped, since the emitter places the line.
- **The before-and-after benches** are 10,000 particles from one emitter, 100 emitters of 100, and 2,000 bouncing particles over a 64×64 tile map.
