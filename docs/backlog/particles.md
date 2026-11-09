# Particles: speed and limits

## Measured

### Before (the scene-driven system, main 74fd1521)

Machine: Apple M5 Pro, 18 cores, shared with other work. Bench: `cargo bench --bench particles`, 1920x1080 headless, 1,000 interleaved frames over an empty frame, the GPU drained outside the measure, each scene warmed up 240 frames first. The scenes: `particles_10k` is one emitter holding 9,916 particles (cap 10,000, rate 5,000, lifetime 2); `particles_100x100` is 100 emitters holding 9,900 (rate 50, max 100 each); `particles_collide_2k` is one emitter holding 1,983 bouncing on a 64x64 tile map (16 px tiles, border and 8 platforms; its cap is raised to 2,000, as the default 1,000 would hold half).

One-minute load before each run (`uptime`), every one above 3 and so flagged: run 1 8.18, run 2 6.07, run 3 4.84. The load fell from run to run and the figures did not move with it.

Each cell is the step's median and 95th percentile over the empty frame's median, in microseconds (an empty frame's step: about 104).

| Bench | Run 1 | Run 2 | Run 3 |
|-------|-------|-------|-------|
| `particles_10k` | 675.8 / 741.8 | 668.7 / 749.6 | 678.3 / 738.2 |
| `particles_100x100` | 704.2 / 775.1 | 704.3 / 790.0 | 714.0 / 783.1 |
| `particles_collide_2k` | 287.2 / 329.7 | 286.6 / 332.0 | 290.2 / 328.3 |

### After (the engine-driven system, this branch)

Machine: the same Apple M5 Pro, 18 cores, shared with other work. Bench: `cargo bench --bench particles`, the same three scenes on the new API (live at the end, as in Before: `particles_10k` 9,916, `particles_100x100` 9,900, `particles_collide_2k` 1,983), 1920x1080 headless, 1,000 interleaved frames over an empty frame, the GPU drained outside the measure, each scene warmed up 240 frames first.

Before and After were measured in one session, as the Before table above ran at a higher load: the scene-driven system's bench (at `d127c8ab`, built from a source export into its own target directory) and this one, three rounds, run alternately (old then new, new then old, old then new), so a drifting load reaches both alike. One-minute load before each run (`uptime`): round 1 old 6.35, new 5.31; round 2 new 4.99, old 3.83; round 3 old 5.51, new 5.22. Every one is above 3 and so flagged; the first had the just-finished build in it, and nothing waited for a quiet machine.

Each cell is the step's median and 95th percentile over its own engine's empty frame's median, in microseconds (an empty frame's step, old 108.9, 120.8 and 110.4; new 101.4, 105.9 and 109.8).

| Bench | Before (rounds 1 to 3) | After (rounds 1 to 3) | Median of rounds, Before to After |
|-------|------------------------|-----------------------|-----------------------------------|
| `particles_10k` | 667.5 / 804.5, 699.6 / 892.2, 688.6 / 776.5 | 759.7 / 868.5, 779.8 / 845.4, 802.3 / 988.1 | 688.6 to 779.8 (+91.2, +13.2%) |
| `particles_100x100` | 697.8 / 844.9, 733.0 / 967.3, 720.6 / 815.2 | 356.3 / 460.8, 362.1 / 418.2, 374.4 / 563.3 | 720.6 to 362.1 (-358.5, -49.8%) |
| `particles_collide_2k` | 281.8 / 372.7, 295.3 / 460.6, 293.0 / 352.9 | 208.2 / 284.0, 212.8 / 249.1, 221.8 / 363.5 | 293.0 to 212.8 (-80.2, -27.4%) |

The spec's targets, on the medians of the three rounds, and the same frames against the scene-driven system's. Beside them, with no target, the particle systems' own time in the frame (`HeadlessEngine::particle_systems_time`: the profiler's `cpu:particles`, the fixed step and the unscaled pass, and `render:particles`, the drawer's `collect`) and the frame's render step over the empty frame's (medians of the round medians):

| Bench | Target | After | Spec's target | Against the scene-driven system | `cpu:particles` | `render:particles` | Render step over empty |
|-------|--------|-------|----------------|----------------------------------|-----------------|---------------------|------------------------|
| `particles_10k` | 1,000 µs | 779.8 µs | met | **missed** (After above Before by 13.2%, in every round) | 522.1 µs | 99.6 µs | +261.8 µs |
| `particles_100x100` | 1,200 µs | 362.1 µs | met | met (-49.8%) | 106.4 µs | 102.0 µs | +262.8 µs |
| `particles_collide_2k` | 1,000 µs | 212.8 µs | met | met (-27.4%) | 145.2 µs | 19.3 µs | +68.2 µs |

The `particles_10k` frame misses the scene-driven system: one emitter of 10,000 particles spends 522 µs in `cpu:particles`, about 53 ns per particle per step, and 100 µs building its instances; the old system's whole frame was 689 µs. Criterion's back-to-back medians (GPU-paced, the three rounds) agree: `particles_10k` old 1,011.0, 1,000.4 and 1,006.3 µs, new 1,039.8, 1,050.3 and 1,049.3; `particles_100x100` old 859.9, 845.8 and 854.8, new 508.1, 496.8 and 489.3; `particles_collide_2k` old 410.6, 411.2 and 413.5, new 337.1, 343.5 and 335.6. What the scopes show of the miss: raising the particles' lifetime to 1,000 s (none die and none spawn once the 10,000 are out; a temporary change to the bench, not kept, run twice beside the same bench unchanged) takes the 10k scene's `cpu:particles` from about 534 µs to about 151, and its frame from about 805 µs to about 416, so about 380 µs of the 534 is the emit-and-remove path: the 83 particles spawned and the 83 removed each step, the in-order compaction of the pool (`Pool::retain`) moving nearly every survivor each step, since the oldest die first. No optimisation was started for the miss.

No emitters (`particles_idle`): the particle systems' own work with no emitters (the fixed step, the unscaled pass and the drawer, run outside a frame by `HeadlessEngine::idle_particle_systems`, 100 batches of 10,000 runs each, the first left out), the median batch's time a run, 73.0, 72.0 and 74.0 ns in the three rounds (spread of each round's batches at most 71 to 102), against the target of 1 µs: met. (Read inside a real empty frame, the same work with the systems' clock reads around it is about 0.37 µs; the idle measure leaves those reads out. A criterion run of the same call made singly, its own clock reads in it, was 97 to 100 ns.)

Allocations (`tests/particles_alloc.rs`, 20 steady emitters, 18 plain with an `on_death` sub-effect and 2 colliding over a tile map's parts with `on_collision`/`on_death`, and a burst effect played with `play_at` every 10 frames, 120 frames warmed up, 120 measured): the measured frames' 418 allocations each are the same 241 the empty frame makes of the engine's own, site by site, and 177 inside the GPU API (asserted none from the particle code's own path); none of the engine's own came from the particle code, and none from the ECS's spawn or despawn code (the one-shots' entities coming and going, printed and not asserted, made none at all). The test names the site of any it finds (checked by planting one in `Pass::step_emitter` and one in the `emitter_inserted` hook: each was named). All of this is in a debug build, in the test harness; the frame targets above are release.

Recorded, with no target:

- The prewarm of a 1,000-particle effect (rate 500, lifetime 2, its cycle 2 s, 993 alive after it): `cpu:particles` in the frame it starts, 434.1, 448.5 and 446.6 µs (median over 30 starts each round, the first 3 left out; 95th percentile 451 to 457), and the whole frame over an ordinary one 476.2, 493.3 and 496.2 µs.
- A sample effect file's reload (`examples/demos/particles/assets/fx/sparks.particles.ron`, 619 B of RON, its rate changed each time; no engine, a world with the asset registry): from the reload to the new version in the store, the main thread pumping as fast as it can (so the worker's latency, not a frame's), 11.1, 11.8 and 11.4 µs in the three rounds (median over 30 reloads, the first 3 left out; 95th percentile 14 to 26 µs).

Each guard prints its verdict and fails the run on a miss (`guards::miss`, `guards::verdict`): the three frame targets and the no-emitters target. Against the scene-driven system is recorded here only: that bench no longer exists on this branch, so it cannot be a run-to-run guard.

### Changes the bench made to the engine

Two hooks, `#[doc(hidden)]` in `src/core/testing/headless.rs`: `HeadlessEngine::particle_systems_time` (the last frame's `cpu:particles` and `render:particles`, from `ParticleTimes` in the store, written by the two systems and around the drawer's `collect`) and `HeadlessEngine::idle_particle_systems(calls)`, which runs the systems' bodies (`run_particle_step`, `run_unscaled_step`, split out of `step_particles` and `step_unscaled_particles`) and the drawer's `collect` outside a frame.

The timers cost the frame two clock reads around each of the two systems and around the drawer, and a store write for each; they are the only cost the systems and the drawer carry for this, and the frame's figures above include it.

## Limits

Each was checked against the code or the module notes (`src/core/particles/AGENTS.md`) before it was written here.

- **The simulation is on the CPU**, one emitter after another on the main thread, in the fixed step: no compute shader (WebGL2 has none, and the web is a target) and no threads. The budget (20,000 live particles by default, `EngineBuilder::particle_budget`) bounds it; the figures above are what it costs on this machine.
- **Particles collide in ground space.** Colliders are where their entities stand, so an emitter on a storey emits where it draws (lifted) but its particles fly, collide and are attracted in ground space, meeting colliders a storey's height below the drawn surface. A storey's edge is no surface (the module notes, "On a floor").
- **`Local` effects do not collide.** `ParticleEffect::problems` reports it and `sanitized` drops the collision, so a `Local` main effect's particles never collide and it never fires `on_collision` (its `on_death` still plays), and a `Local` sub-effect does not collide either.
- **Sub-emitters go one level deep.** A sub-effect's own sub-emitters are ignored, with a warning once per compiled version; a file naming such a sub-effect also warns when it loads.
- **Each particle is clipped by the GPU, not culled on the CPU.** The drawer culls whole runs: an emitter whose box (or a sub-effect's, its own) meets a camera's view on its layers submits every particle it holds, so a large emitter mostly off screen still pays for every instance and the overdraw of what is on screen. There is no per-particle cull.
- **The cull box is grown only by the largest a particle can draw now:** its effect's largest size times its size curve's largest value. Glow and outline padding (`with_effect` sprite effects drawn past the quad) are not counted, and neither are particles spawned larger under an earlier compiled version of the effect, so their edges can be cut at the view's edge.
- **A `Local` emitter not stepped can be culled on screen.** Its box is rebuilt only when it steps, so one held by a scene, or at speed 0, whose entity moves away draws through where it stands (the local frame) but is culled by the box from its last step.
- **Prewarm is paid at once.** A prewarmed effect runs `min(duration, 10)` s of steps (at most 600, longer ones past that) inside the first step of its emitter, so the frame it appears in pays its whole prewarm (about 450 µs for 1,000 particles over a 2 s cycle, recorded above; it scales with the particles and the steps).
- **Effect files on the web must be embedded.** The web has no asset folder and no hot reload: `.particles.ron` files reach the game only through a source of embedded bytes, as the particles demo's `embedded.rs` does, and that list is kept by hand.
- **A fast emitter's stream follows its fixed steps, not the frames between them.** Spawns spread along the straight line between where the emitter stood at its last two fixed steps, the newest at the step's end. The particles draw blended between their last two steps; an `Interpolated` emitter draws partway along its step, so the newest particles sit up to a step of its movement ahead of it, and an emitter that is not `Interpolated` draws at the step's end, where the newest are. Nothing finer than the fixed step is seen, so a curve inside one step, or a transform moved every rendered frame, is a chord. A teleport (`skip_interpolation`) spreads nothing.
- **An unscaled emitter taking several steps in one frame gets a lumpy inherited velocity:** the whole frame's move lands in its first step and the others see none (`Frame::inherited`).
- **A mirrored emitter that turns or scales between steps draws its `Local` particles unmirrored between steps** (`GlobalTransform::lerp`, which `render_global` uses, drops the mirror when the two steps' matrices differ; each step's end is right).
- **One distant or huge collider coarsens the collision grid.** The grid's cell side follows the mean edge length (or collider size) and doubles until at most 65,536 cells cover every gathered collider's span, so a single far-off collider widens every cell of the pass and each particle's query finds more candidates. Results stay exact; only the cost moves.
- **Smaller ones.** A `play_at` whose effect never loads leaves its emitter entity waiting (no warning, no despawn). `ParticlesView::clear` leaves a held emitter's `alive` stale until it next steps. An unscaled `Interpolated` emitter's stream keeps a teleport's "no spread" while the game clock is paused, because the fixed step's snapshot that clears `PreviousGlobalTransform.skip` does not run. A turbulence attractor's time advances with the game-time pass only (when it has an emitter to step), so an unscaled emitter's turbulence freezes while paused or with no game-time emitter stepping. A sub pool names its effect by id, not by a kept handle, so the particles a parent played last keep going after it is repointed only while something else still holds that effect.
